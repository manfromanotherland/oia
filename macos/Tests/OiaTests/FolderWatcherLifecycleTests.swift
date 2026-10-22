// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import XCTest

final class FolderWatcherLifecycleTests: XCTestCase {
    func testSlowSetupAndInvalidationNeverBlockTheCaller() async {
        let enteredSetup = expectation(description: "native setup entered")
        let disposed = expectation(description: "canceled stream disposed")
        let changes = expectation(description: "canceled setup cannot emit changes")
        changes.isInverted = true
        let gate = DispatchSemaphore(value: 0)
        let started = Date()
        let watcher = FolderWatcher(paths: [], onChange: { _ in changes.fulfill() }) { _, _, callback in
            XCTAssertFalse(Thread.isMainThread)
            enteredSetup.fulfill()
            _ = gate.wait(timeout: .now() + 2)
            callback(.full)
            return TestFolderWatcherStream(start: {
                XCTFail("A canceled setup must never start monitoring")
                return true
            }, stop: {
                XCTAssertFalse(Thread.isMainThread)
                disposed.fulfill()
            })
        }
        XCTAssertLessThan(Date().timeIntervalSince(started), 0.25)
        await fulfillment(of: [enteredSetup], timeout: 1)
        let invalidating = Date()
        watcher.invalidate()
        XCTAssertLessThan(Date().timeIntervalSince(invalidating), 0.25)
        gate.signal()
        await fulfillment(of: [disposed, changes], timeout: 0.5)
    }

    func testSuccessfulSetupReconcilesGapAndSuppressesLateCallbacks() async {
        let activated = expectation(description: "active monitoring closes setup gap")
        let disposed = expectation(description: "stream stopped once")
        let lateChange = expectation(description: "late native callback suppressed")
        lateChange.isInverted = true
        let watcher = FolderWatcher(paths: [], onChange: { change in
            XCTAssertFalse(Thread.isMainThread)
            if change == .full { activated.fulfill() } else { lateChange.fulfill() }
        }) { _, _, callback in
            TestFolderWatcherStream(start: {
                XCTAssertFalse(Thread.isMainThread)
                return true
            }, stop: {
                XCTAssertFalse(Thread.isMainThread)
                callback(ChangeForTest.late)
                disposed.fulfill()
            })
        }
        await fulfillment(of: [activated], timeout: 1)
        watcher.invalidate()
        watcher.invalidate()
        await fulfillment(of: [disposed, lateChange], timeout: 0.5)
    }

    func testDroppingWatcherDisposesActiveStream() async {
        let activated = expectation(description: "stream active")
        let disposed = expectation(description: "stream disposed on owner release")
        var watcher: FolderWatcher? = FolderWatcher(paths: [], onChange: { _ in activated.fulfill() }) {
            _, _, _ in
            TestFolderWatcherStream(start: { true }, stop: {
                XCTAssertFalse(Thread.isMainThread)
                disposed.fulfill()
            })
        }
        await fulfillment(of: [activated], timeout: 1)
        weak var releasedWatcher = watcher
        watcher = nil
        XCTAssertNil(releasedWatcher, "The native callback context must not retain its watcher")
        await fulfillment(of: [disposed], timeout: 1)
    }

    func testFailedStartDisposesWithoutPublishingActiveMonitoring() async {
        let disposed = expectation(description: "failed start disposed")
        let changes = expectation(description: "failed start has no startup reconciliation")
        changes.isInverted = true
        let watcher = FolderWatcher(paths: [], onChange: { _ in changes.fulfill() }) { _, _, _ in
            TestFolderWatcherStream(start: { false }, stop: { disposed.fulfill() })
        }
        await fulfillment(of: [disposed, changes], timeout: 0.5)
        watcher.invalidate()
    }

    func testRecoveryWatcherWaitsForRealChangesWithoutSyntheticStartupRetry() async {
        let realChange = expectation(description: "real filesystem callback delivered")
        let startupRetry = expectation(description: "recovery has no synthetic startup retry")
        startupRetry.isInverted = true
        let watcher = FolderWatcher(paths: [], reconcileOnStart: false, onChange: { change in
            if change == .full { startupRetry.fulfill() } else { realChange.fulfill() }
        }) { _, _, callback in
            TestFolderWatcherStream(start: {
                callback(ChangeForTest.late)
                return true
            }, stop: {})
        }
        await fulfillment(of: [realChange, startupRetry], timeout: 0.5)
        watcher.invalidate()
    }
}

private final class TestFolderWatcherStream: FolderWatcherStream, @unchecked Sendable {
    private let onStart: @Sendable () -> Bool
    private let onStop: @Sendable () -> Void

    init(start: @escaping @Sendable () -> Bool, stop: @escaping @Sendable () -> Void) {
        onStart = start
        onStop = stop
    }

    func start() -> Bool { onStart() }
    func stop() { onStop() }
}

private enum ChangeForTest {
    static let late = FolderWatcher.Change(paths: ["late-change"])
}
