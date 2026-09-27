// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// Pure filtering and one-motion selection rules used by the library board.
final class ComposedFilterTests: XCTestCase {
    func testMatchesUsesTheSelectedScope() {
        let image = makeReadingRow(kind: .image)
        XCTAssertTrue(ComposedFilter.matches(image, scope: .all))
        XCTAssertTrue(ComposedFilter.matches(image, scope: .images))
        XCTAssertFalse(ComposedFilter.matches(image, scope: .videos))
        XCTAssertFalse(ComposedFilter.matches(image, scope: .articles))
    }

    func testAllMatchesLegacyArchivedRows() {
        let row = makeReadingRow(archived: true, favorite: true)
        XCTAssertTrue(ComposedFilter.matches(row, scope: .all))
    }

    func testSelectionAfterRemovingPrefersNextThenPreviousThenNil() {
        let rows = [rowWithId("a"), rowWithId("b"), rowWithId("c")]
        XCTAssertEqual(ComposedFilter.selectionAfterRemoving(at: 0, from: rows), "b")
        XCTAssertEqual(ComposedFilter.selectionAfterRemoving(at: 2, from: rows), "b")
        XCTAssertNil(ComposedFilter.selectionAfterRemoving(at: 0, from: [rowWithId("only")]))
    }

    private func rowWithId(_ id: String) -> ReadingRow {
        var row = makeReadingRow()
        row.id = id
        return row
    }
}
