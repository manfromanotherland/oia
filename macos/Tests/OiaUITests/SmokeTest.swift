// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// Bring-up smoke test for the sidebar-free board: load every seeded card,
/// open one, search to a single result, and delete it.
final class SmokeTest: UITestCase {
    func testBoardEscapeAndUnselectedCardContextDelete() throws {
        try launchApp(articles: Fixtures.standardCorpus)

        list.select(Fixtures.Ids.rust)
        XCTAssertTrue(list.row(Fixtures.Ids.rust).isSelected)
        keyboard.escape()
        XCTAssertTrue(wait { !list.row(Fixtures.Ids.rust).isSelected },
                      "Escape clears the card selection")

        list.invokeContextMenu(on: Fixtures.Ids.swift, item: "Delete")
        list.confirmDelete()
        XCTAssertTrue(list.row(Fixtures.Ids.swift).waitDisappears(),
                      "An unselected card can be deleted from its context menu")
    }

    func testLaunchBoardOpenSearchAndDelete() throws {
        try launchApp(articles: Fixtures.standardCorpus)

        // Legacy read/archive metadata does not remove cards from the board.
        XCTAssertTrue(list.waitForRowCount(Fixtures.standardCorpus.count), "all cards loaded")
        XCTAssertTrue(list.row(Fixtures.Ids.archived).waitExists(), "legacy archived card is visible")
        XCTAssertTrue(app.byId(A11y.Filter.group).exists, "filter control is available")

        // A single click selects without opening; a double click opens.
        list.select(Fixtures.Ids.rust)
        XCTAssertTrue(list.row(Fixtures.Ids.rust).isSelected, "Rust card should be selected")
        XCTAssertFalse(
            reader.closeButton.waitForExistence(timeout: 0.5),
            "Single click should leave the board visible"
        )
        XCTAssertTrue(list.row(Fixtures.Ids.rust).isHittable, "Selected card should remain usable")

        list.open(Fixtures.Ids.rust)
        XCTAssertTrue(reader.title.waitExists(), "Reader title should appear")
        XCTAssertEqual(reader.titleText, "Understanding Rust Ownership")
        reader.close()

        // "ownership" appears only in the Rust article.
        list.search(Fixtures.Search.activeTerm)
        XCTAssertTrue(list.row(Fixtures.Ids.rust).waitExists(), "Rust article should match")
        XCTAssertTrue(list.row(Fixtures.Ids.swift).waitDisappears(), "Non-matching article should be gone")
        XCTAssertTrue(list.waitForRowCount(1), "Search should narrow to exactly one row")
        XCTAssertEqual(list.orderedRowIds, [Fixtures.Ids.rust])

        // Delete the filtered card and verify both the board and source-of-truth file.
        list.open(Fixtures.Ids.rust)
        reader.delete()
        list.confirmDelete()
        XCTAssertTrue(list.waitForRowCount(0), "Deleted card should leave the filtered board")
        XCTAssertTrue(wait { !library.articleExists(id: Fixtures.Ids.rust) }, "Deleted card file should be removed")
    }
}
