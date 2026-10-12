// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class PendingTagEditsTests: XCTestCase {
    func testCompletedAddCannotRestoreAStillPendingRemoval() {
        var pending = PendingTagEdits()
        let add = pending.reserve(tag: "demo", ids: ["one"], applies: true, librarySession: 42)
        let remove = pending.reserve(tag: "demo", ids: ["one"], applies: false, librarySession: 42)
        XCTAssertTrue(pending.apply(to: row("one", tags: []), librarySession: 42).tags.isEmpty)

        pending.complete(add)
        let afterAdd = row("one", tags: ["demo"])

        XCTAssertTrue(pending.apply(to: afterAdd, librarySession: 42).tags.isEmpty)
        XCTAssertTrue(pending.hasEdits(librarySession: 42))
        pending.complete(remove)
        XCTAssertFalse(pending.hasEdits(librarySession: 42))
        XCTAssertTrue(pending.apply(to: row("one", tags: []), librarySession: 42).tags.isEmpty)
    }

    func testEarlierCompletionKeepsALaterAddedTagVisible() {
        var pending = PendingTagEdits()
        let first = pending.reserve(tag: "first", ids: ["one"], applies: true, librarySession: 42)
        pending.reserve(tag: "second", ids: ["one"], applies: true, librarySession: 42)

        pending.complete(first)
        let afterFirst = row("one", tags: ["keep", "first"])

        XCTAssertEqual(
            pending.apply(to: afterFirst, librarySession: 42).tags,
            ["keep", "first", "second"]
        )
    }

    func testFailedRemovalRestoresAuthoritativeMachineOwnershipOnlyAfterCompletion() {
        let stored = row("one", tags: ["video", "keep"], machineTags: ["video"])
        var pending = PendingTagEdits()
        let remove = pending.reserve(tag: "VIDEO", ids: ["one"], applies: false, librarySession: 42)

        let whilePending = pending.apply(to: stored, librarySession: 42)
        XCTAssertEqual(whilePending.tags, ["keep"])
        XCTAssertTrue(whilePending.machineTags.isEmpty)

        pending.complete(remove)

        XCTAssertEqual(pending.apply(to: stored, librarySession: 42), stored)
    }

    func testFailedAddFallsBackToAuthoritativeTagsAfterCompletion() {
        let stored = row("one", tags: ["keep"])
        var pending = PendingTagEdits()
        let add = pending.reserve(tag: "demo", ids: ["one"], applies: true, librarySession: 42)
        XCTAssertEqual(pending.apply(to: stored, librarySession: 42).tags, ["keep", "demo"])

        pending.complete(add)

        XCTAssertEqual(pending.apply(to: stored, librarySession: 42), stored)
    }

    func testUnselectedRowsRemainUnchangedWhileEveryFrozenTargetGetsTheEdit() {
        var pending = PendingTagEdits()
        pending.reserve(tag: "demo", ids: ["one", "two"], applies: true, librarySession: 42)
        let unrelated = row("other", tags: ["video"], machineTags: ["video"])

        XCTAssertEqual(pending.apply(to: unrelated, librarySession: 42), unrelated)
        XCTAssertEqual(
            pending.apply(to: row("one", tags: ["first"]), librarySession: 42).tags,
            ["first", "demo"]
        )
        XCTAssertEqual(
            pending.apply(to: row("two", tags: ["second"]), librarySession: 42).tags,
            ["second", "demo"]
        )
    }

    func testSessionsDoNotShareEditsEvenWhenReadingIDsMatch() {
        let stored = row("one", tags: ["keep"])
        var pending = PendingTagEdits()
        let old = pending.reserve(tag: "old", ids: ["one"], applies: true, librarySession: 41)
        pending.reserve(tag: "new", ids: ["one"], applies: true, librarySession: 42)

        XCTAssertEqual(pending.apply(to: stored, librarySession: 41).tags, ["keep", "old"])
        XCTAssertEqual(pending.apply(to: stored, librarySession: 42).tags, ["keep", "new"])
        XCTAssertEqual(pending.apply(to: stored, librarySession: 43), stored)
        XCTAssertFalse(pending.hasEdits(librarySession: 43))

        pending.complete(old)

        XCTAssertFalse(pending.hasEdits(librarySession: 41))
        XCTAssertTrue(pending.hasEdits(librarySession: 42))
        XCTAssertEqual(pending.apply(to: stored, librarySession: 42).tags, ["keep", "new"])
    }

    func testAddingAnExistingMachineTagImmediatelyTransfersUserOwnership() {
        let stored = row("one", tags: ["VIDEO", "keep"], machineTags: ["VIDEO"])
        var pending = PendingTagEdits()
        pending.reserve(tag: " video ", ids: ["one"], applies: true, librarySession: 42)

        let presented = pending.apply(to: stored, librarySession: 42)

        XCTAssertEqual(presented.tags, ["video", "keep"])
        XCTAssertTrue(presented.machineTags.isEmpty)
    }

    func testRemoveThenAddReplaysInRequestedOrderWithoutDuplicatingTheTag() {
        var pending = PendingTagEdits()
        pending.reserve(tag: "demo", ids: ["one"], applies: false, librarySession: 42)
        pending.reserve(tag: "DEMO", ids: ["one"], applies: true, librarySession: 42)

        let presented = pending.apply(to: row("one", tags: ["demo", "keep"]), librarySession: 42)

        XCTAssertEqual(presented.tags, ["keep", "DEMO"])
    }

    func testCompletingOneReservationDoesNotDiscardAnotherOrAnUnknownID() {
        let stored = row("one", tags: [])
        var pending = PendingTagEdits()
        let first = pending.reserve(tag: "first", ids: ["one"], applies: true, librarySession: 42)
        let second = pending.reserve(tag: "second", ids: ["one"], applies: true, librarySession: 42)
        XCTAssertNotEqual(first, second)

        pending.complete(second)
        pending.complete(UInt64.max)

        XCTAssertEqual(pending.apply(to: stored, librarySession: 42).tags, ["first"])
        XCTAssertTrue(pending.hasEdits(librarySession: 42))
    }

    func testRevisionTracksReservationAndSettlementButIgnoresRepeatedCompletions() {
        var pending = PendingTagEdits()
        XCTAssertEqual(pending.revision, 0)
        let first = pending.reserve(tag: "first", ids: ["one"], applies: true, librarySession: 42)
        XCTAssertEqual(pending.revision, 1)
        let second = pending.reserve(tag: "second", ids: ["one"], applies: true, librarySession: 42)
        XCTAssertEqual(pending.revision, 2)

        pending.complete(first)

        XCTAssertEqual(pending.revision, 3)
        XCTAssertEqual(pending.apply(to: row("one", tags: ["first"]), librarySession: 42).tags, ["first", "second"])
        pending.complete(first)
        pending.complete(UInt64.max)
        XCTAssertEqual(pending.revision, 3)

        pending.complete(second)

        XCTAssertEqual(pending.revision, 4)
        XCTAssertFalse(pending.hasEdits(librarySession: 42))
    }

    private func row(_ id: String, tags: [String], machineTags: [String] = []) -> ReadingRow {
        var result = makeReadingRow()
        result.id = id
        result.tags = tags
        result.machineTags = machineTags
        return result
    }
}
