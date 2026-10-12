// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

@MainActor
final class TagPersistenceQueueTests: XCTestCase {
    func testRapidAddAndRemoveReserveTheirActionOrder() async {
        let queue = TagPersistenceQueue()
        let addGate = TagPersistenceGate()
        var edits: [String] = []
        let add = queue.enqueue {
            edits.append("add demo")
            await addGate.suspend()
            return 1
        }
        let remove = queue.enqueue {
            edits.append("remove demo")
            return 2
        }

        await addGate.waitUntilSuspended()
        XCTAssertTrue(queue.isBusy)
        XCTAssertEqual(edits, ["add demo"])
        addGate.resume()
        let addedCount = await add.value
        let removedCount = await remove.value

        XCTAssertEqual(addedCount, 1)
        XCTAssertEqual(removedCount, 2)
        XCTAssertEqual(edits, ["add demo", "remove demo"])
        XCTAssertFalse(queue.isBusy)
    }

    func testOlderCompletionCannotReleaseNewerReservations() async {
        let queue = TagPersistenceQueue()
        let firstGate = TagPersistenceGate()
        let secondGate = TagPersistenceGate()
        var completed: [String] = []
        let first = queue.enqueue {
            await firstGate.suspend()
            completed.append("first")
            return 1
        }
        let second = queue.enqueue {
            await secondGate.suspend()
            completed.append("second")
            return 2
        }

        await firstGate.waitUntilSuspended()
        firstGate.resume()
        await secondGate.waitUntilSuspended()
        _ = await first.value
        XCTAssertTrue(queue.isBusy, "Finishing an older edit must keep the newer queue reservation")
        let third = queue.enqueue {
            completed.append("third")
            return 3
        }

        secondGate.resume()
        let secondCount = await second.value
        let thirdCount = await third.value
        XCTAssertEqual(secondCount, 2)
        XCTAssertEqual(thirdCount, 3)
        XCTAssertEqual(completed, ["first", "second", "third"])
        XCTAssertFalse(queue.isBusy)
    }
}

/// Lets tests observe and release a precise suspension without timers or sleeps.
@MainActor
private final class TagPersistenceGate {
    private var suspension: CheckedContinuation<Void, Never>?
    private var observer: CheckedContinuation<Void, Never>?

    func suspend() async {
        await withCheckedContinuation { continuation in
            suspension = continuation
            observer?.resume()
            observer = nil
        }
    }

    func waitUntilSuspended() async {
        guard suspension == nil else { return }
        await withCheckedContinuation { observer = $0 }
    }

    func resume() {
        suspension?.resume()
        suspension = nil
    }
}
