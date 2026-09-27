// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

@MainActor
final class VideoPlaybackSchedulerTests: XCTestCase {
    func testDefaultAdmissionAllowsEveryVisibleCard() async throws {
        let scheduler = VideoPlaybackScheduler(startSpacing: .zero)
        var leases: [UUID] = []
        for _ in 0 ..< 12 {
            leases.append(try await acquire(scheduler))
        }
        XCTAssertEqual(scheduler.state, .init(active: 12, waiting: 0))
        for lease in leases { scheduler.release(lease) }
    }

    func testAdmissionIsBoundedAndWaitersAdvanceInOrder() async throws {
        let scheduler = VideoPlaybackScheduler(limit: 1, startSpacing: .zero)
        let first = try await acquire(scheduler)
        let second = Task { await scheduler.acquire() }
        await assertState(scheduler) { $0.waiting == 1 }
        let third = Task { await scheduler.acquire() }
        await assertState(scheduler) { $0.waiting == 2 }
        XCTAssertEqual(scheduler.state.active, 1)

        scheduler.release(first)
        let secondValue = await second.value
        let secondLease = try XCTUnwrap(secondValue)
        XCTAssertEqual(scheduler.state, .init(active: 1, waiting: 1))
        scheduler.release(secondLease)
        let thirdValue = await third.value
        let thirdLease = try XCTUnwrap(thirdValue)
        scheduler.release(thirdLease)
        XCTAssertEqual(scheduler.state, .init(active: 0, waiting: 0))
    }

    func testCancelledWaitingCardDoesNotConsumeAPlaybackSlot() async throws {
        let scheduler = VideoPlaybackScheduler(limit: 1, startSpacing: .zero)
        let first = try await acquire(scheduler)
        let cancelled = Task { await scheduler.acquire() }
        await assertState(scheduler) { $0.waiting == 1 }
        cancelled.cancel()
        let cancelledValue = await cancelled.value
        XCTAssertNil(cancelledValue)
        await assertState(scheduler) { $0.waiting == 0 }
        scheduler.release(first)
        let next = try await acquire(scheduler)
        XCTAssertEqual(scheduler.state.active, 1)
        scheduler.release(next)
    }

    func testCancellationRacingAdmissionReturnsItsLease() async throws {
        for _ in 0 ..< 20 {
            let scheduler = VideoPlaybackScheduler(limit: 1, startSpacing: .zero)
            let first = try await acquire(scheduler)
            let pending = Task { await scheduler.acquire() }
            await assertState(scheduler) { $0.waiting == 1 }
            scheduler.release(first)
            pending.cancel()
            if let lease = await pending.value {
                scheduler.release(lease)
            }
            await assertState(scheduler) { $0.active == 0 && $0.waiting == 0 }
        }
    }

    func testStartTurnsAreSpacedAndCancellationDoesNotStartWork() async {
        let scheduler = VideoPlaybackScheduler(startSpacing: .milliseconds(20))
        let firstStarted = await scheduler.waitForStartTurn()
        XCTAssertTrue(firstStarted)
        let started = ContinuousClock.now
        let secondStarted = await scheduler.waitForStartTurn()
        XCTAssertTrue(secondStarted)
        XCTAssertGreaterThanOrEqual(started.duration(to: .now), .milliseconds(15))
        let pending = Task { await scheduler.waitForStartTurn() }
        pending.cancel()
        let cancelledStarted = await pending.value
        XCTAssertFalse(cancelledStarted)
    }

    private func acquire(_ scheduler: VideoPlaybackScheduler) async throws -> UUID {
        let lease = await scheduler.acquire()
        return try XCTUnwrap(lease)
    }

    private func assertState(
        _ scheduler: VideoPlaybackScheduler,
        file: StaticString = #filePath,
        line: UInt = #line,
        predicate: (VideoPlaybackScheduler.State) -> Bool
    ) async {
        for _ in 0 ..< 100 {
            if predicate(scheduler.state) {
                return
            }
            await Task.yield()
        }
        XCTFail("Scheduler did not reach the expected state", file: file, line: line)
    }
}
