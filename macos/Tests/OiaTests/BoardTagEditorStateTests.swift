// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class BoardTagEditorStateTests: XCTestCase {
    func testOneCardStartsWithUserAndMachineTags() {
        let state = editor([row("one", tags: ["demo", "video"], machineTags: ["video"])])

        XCTAssertEqual(state.tags, [
            .init(name: "demo", isMachine: false),
            .init(name: "video", isMachine: true)
        ])
        XCTAssertEqual(state.commonUserTags, ["demo"])
    }

    func testSeveralCardsInitiallyHideTheirExistingTags() {
        let state = editor([
            row("one", tags: ["shared", "first"]),
            row("two", tags: ["shared", "second"])
        ])

        XCTAssertTrue(state.tags.isEmpty)
        XCTAssertEqual(state.commonUserTags, ["shared"])
    }

    func testAddAppearsBeforePersistenceCompletesAndKeepsExistingTags() throws {
        var state = editor([row("one", tags: ["existing"])])

        let edit = try XCTUnwrap(state.addTag(" demo "))

        XCTAssertEqual(state.tags.map(\.name), ["existing", "demo"])
        XCTAssertEqual(state.nextEdit, edit)
        XCTAssertEqual(edit.tag, "demo")
        XCTAssertTrue(edit.applies)
    }

    func testRemoveDisappearsBeforePersistenceCompletes() throws {
        var state = editor([row("one", tags: ["demo", "keep"])])

        let edit = try XCTUnwrap(state.removeTag("DEMO"))

        XCTAssertEqual(state.tags.map(\.name), ["keep"])
        XCTAssertEqual(state.nextEdit, edit)
        XCTAssertFalse(edit.applies)
    }

    func testAddingMachineTagImmediatelyTransfersUserOwnership() throws {
        var state = editor([row("one", tags: ["VIDEO"], machineTags: ["VIDEO"])])

        let edit = try XCTUnwrap(state.addTag("video"))

        XCTAssertEqual(state.tags, [.init(name: "video", isMachine: false)])
        XCTAssertEqual(state.commonUserTags, ["video"])
        state.complete(edit, rows: [row("one", tags: ["video"])])
        XCTAssertEqual(state.tags, [.init(name: "video", isMachine: false)])
    }

    func testFailedAddReconcilesToAuthoritativeTags() throws {
        let original = row("one", tags: ["keep"])
        var state = editor([original])
        let edit = try XCTUnwrap(state.addTag("demo"))
        XCTAssertEqual(state.tags.map(\.name), ["keep", "demo"])

        state.complete(edit, rows: [original])

        XCTAssertEqual(state.tags.map(\.name), ["keep"])
        XCTAssertNil(state.nextEdit)
    }

    func testFailedRemovalRestoresMachineTagFromAuthoritativeSnapshot() throws {
        let original = row("one", tags: ["video"], machineTags: ["video"])
        var state = editor([original])
        let edit = try XCTUnwrap(state.removeTag("video"))
        XCTAssertTrue(state.tags.isEmpty)

        state.complete(edit, rows: [original])

        XCTAssertEqual(state.tags, [.init(name: "video", isMachine: true)])
    }

    func testUnrelatedRefreshRetainsAllPendingEdits() throws {
        var state = editor([row("one", tags: ["remove"])])
        let add = try XCTUnwrap(state.addTag("demo"))
        let remove = try XCTUnwrap(state.removeTag("remove"))

        state.updateRows([row("one", tags: ["remove", "external"])])

        XCTAssertEqual(state.tags.map(\.name), ["external", "demo"])
        XCTAssertEqual(state.nextEdit, add)
        state.complete(add, rows: [row("one", tags: ["remove", "external", "demo"])])
        XCTAssertEqual(state.tags.map(\.name), ["external", "demo"])
        XCTAssertEqual(state.nextEdit, remove)
    }

    func testRapidAddThenRemoveStaysRemovedWhileOlderAddSettles() throws {
        var state = editor([row("one", tags: ["keep"])])
        let add = try XCTUnwrap(state.addTag("demo"))
        let remove = try XCTUnwrap(state.removeTag("demo"))
        XCTAssertEqual(state.tags.map(\.name), ["keep"])

        state.complete(add, rows: [row("one", tags: ["keep", "demo"])])

        XCTAssertEqual(state.tags.map(\.name), ["keep"])
        XCTAssertEqual(state.nextEdit, remove)
        state.complete(remove, rows: [row("one", tags: ["keep"])])
        XCTAssertEqual(state.tags.map(\.name), ["keep"])
        XCTAssertNil(state.nextEdit)
    }

    func testFailedEarlierWriteDoesNotRemoveALaterOptimisticAdd() throws {
        var state = editor([row("one", tags: ["keep"])])
        let first = try XCTUnwrap(state.addTag("first"))
        let second = try XCTUnwrap(state.addTag("second"))

        state.complete(first, rows: [row("one", tags: ["keep"])])

        XCTAssertEqual(state.tags.map(\.name), ["keep", "second"])
        XCTAssertEqual(state.nextEdit, second)
    }

    func testBulkAddedTagIsRemovableWithoutExposingOrReplacingOtherTags() throws {
        var state = editor([
            row("one", tags: ["first"]),
            row("two", tags: ["second"])
        ])
        let add = try XCTUnwrap(state.addTag("demo"))
        XCTAssertEqual(state.tags.map(\.name), ["demo"])
        state.complete(add, rows: [
            row("one", tags: ["first", "demo"]),
            row("two", tags: ["second", "demo"])
        ])
        let remove = try XCTUnwrap(state.removeTag("demo"))
        XCTAssertTrue(state.tags.isEmpty)

        state.complete(remove, rows: [
            row("one", tags: ["first"]),
            row("two", tags: ["second"])
        ])

        XCTAssertTrue(state.tags.isEmpty)
        XCTAssertNil(state.nextEdit)
        XCTAssertTrue(state.commonUserTags.isEmpty)
    }

    func testPartialBulkFailureReflectsRowsThatActuallyKeptTheAddedTag() throws {
        var state = editor([
            row("one", tags: ["first"]),
            row("two", tags: ["second"])
        ])
        let add = try XCTUnwrap(state.addTag("demo"))

        state.complete(add, rows: [
            row("one", tags: ["first", "demo"]),
            row("two", tags: ["second"])
        ])

        XCTAssertEqual(state.tags.map(\.name), ["demo"])
        XCTAssertTrue(state.commonUserTags.isEmpty)
    }

    func testFilteredOutTargetKeepsItsSnapshotAndReconcilesFromExactRowReload() throws {
        var state = editor([row("one", tags: ["filter", "keep"])])
        let remove = try XCTUnwrap(state.removeTag("filter"))

        state.updateRows([row("newSelection", tags: ["filter"])])

        XCTAssertEqual(state.targetIDs, ["one"])
        XCTAssertEqual(state.librarySession, 42)
        XCTAssertEqual(state.tags.map(\.name), ["keep"])
        state.complete(remove, rows: [row("one", tags: ["keep", "external"])])
        XCTAssertEqual(state.tags.map(\.name), ["keep", "external"])
    }

    func testDeletedTargetDoesNotRetainGhostTagsAfterAuthoritativeReload() throws {
        var state = editor([row("one", tags: ["existing"])])
        let edit = try XCTUnwrap(state.addTag("demo"))

        state.complete(edit, rows: [])

        XCTAssertTrue(state.tags.isEmpty)
        XCTAssertTrue(state.commonUserTags.isEmpty)
        XCTAssertEqual(state.targetIDs, ["one"])
        XCTAssertNil(state.nextEdit)
    }

    func testDeletedBulkTargetDoesNotKeepASessionTagAlive() throws {
        var state = editor([
            row("one", tags: ["first"]),
            row("two", tags: ["second"])
        ])
        let edit = try XCTUnwrap(state.addTag("demo"))

        state.complete(edit, rows: [row("two", tags: ["second"])])

        XCTAssertTrue(state.tags.isEmpty)
        XCTAssertTrue(state.commonUserTags.isEmpty)
    }

    func testUnavailableTargetCanRecoverWithinTheFrozenSession() throws {
        var state = editor([row("one", tags: ["existing"])])
        let edit = try XCTUnwrap(state.addTag("demo"))
        state.complete(edit, rows: [])
        XCTAssertTrue(state.tags.isEmpty)

        state.updateRows([
            row("one", tags: ["recovered"], machineTags: ["recovered"]),
            row("other", tags: ["ignore"])
        ])

        XCTAssertEqual(state.targetIDs, ["one"])
        XCTAssertEqual(state.tags, [.init(name: "recovered", isMachine: true)])
    }

    func testOutOfOrderCompletionCannotDiscardAnEarlierPendingEdit() throws {
        var state = editor([row("one", tags: [])])
        let first = try XCTUnwrap(state.addTag("first"))
        let second = try XCTUnwrap(state.addTag("second"))

        state.complete(second, rows: [row("one", tags: ["second"])])

        XCTAssertEqual(state.nextEdit, first)
        XCTAssertEqual(state.tags.map(\.name), ["first", "second"])
    }

    func testInvalidAddsNeverQueueEditsOrPills() {
        var state = editor([row("one", tags: [])])

        XCTAssertNil(state.addTag("   "))
        XCTAssertNil(state.addTag(String(repeating: "a", count: TagRules.maxLength + 1)))

        XCTAssertNil(state.nextEdit)
        XCTAssertTrue(state.tags.isEmpty)
    }

    private func editor(_ rows: [ReadingRow]) -> BoardTagEditorState {
        BoardTagEditorState(rows: rows, librarySession: 42)
    }

    private func row(_ id: String, tags: [String], machineTags: [String] = []) -> ReadingRow {
        var result = makeReadingRow()
        result.id = id
        result.tags = tags
        result.machineTags = machineTags
        return result
    }
}
