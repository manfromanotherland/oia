// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// Create a tag from the inspector, apply it to a second card, then remove it
/// from both cards without disturbing either reading's other tags.
final class TagOrganizingJourney: UITestCase {
    private static let newTag = "important"

    // One end-to-end organizing walk; asserting each persistence boundary keeps it long.
    // swiftlint:disable:next function_body_length
    func testCreateApplyAndRemoveTags() throws {
        try launchApp(articles: Fixtures.standardCorpus)
        let tag = Self.newTag

        openReading(Fixtures.Ids.minimal)
        XCTAssertEqual(reader.titleText, "Minimal")
        reader.openTagPicker()
        XCTAssertTrue(tagPicker.isVisible, "tag picker opened")
        XCTAssertTrue(tagPicker.row("programming").waitExists(), "existing tags are listed")
        tagPicker.createAndApply(tag)
        tagPicker.done()
        XCTAssertTrue(reader.waitForTag(tag), "#\(tag) appears on the first card")
        XCTAssertTrue(
            waitForFrontmatter(id: Fixtures.Ids.minimal) { $0.tags.contains(tag) },
            "tag written to minimal's frontmatter"
        )

        reader.close()
        openReading(Fixtures.Ids.unicode)
        reader.openTagPicker()
        XCTAssertTrue(tagPicker.row(tag).waitExists(), "created tag is available on another card")
        XCTAssertEqual(tagPicker.orderedRowTags.first, "unicode", "the card's applied tag stays first")
        tagPicker.toggle(tag)
        tagPicker.done()
        XCTAssertTrue(reader.waitForTag(tag), "#\(tag) appears on the second card")
        XCTAssertTrue(
            waitForFrontmatter(id: Fixtures.Ids.unicode) { $0.tags.contains(tag) },
            "tag written to unicode's frontmatter"
        )

        reader.close()
        openReading(Fixtures.Ids.minimal)
        reader.openTagPicker()
        tagPicker.toggle(tag)
        tagPicker.done()
        XCTAssertTrue(
            waitForFrontmatter(id: Fixtures.Ids.minimal) { !$0.tags.contains(tag) },
            "tag removed from minimal's frontmatter"
        )
        reader.close()
        XCTAssertTrue(list.row(Fixtures.Ids.minimal).waitExists(), "minimal remains on the board")

        openReading(Fixtures.Ids.unicode)
        reader.openTagPicker()
        tagPicker.toggle(tag)
        tagPicker.done()
        XCTAssertTrue(
            waitForFrontmatter(id: Fixtures.Ids.unicode) { !$0.tags.contains(tag) },
            "tag removed from unicode's frontmatter"
        )
        XCTAssertTrue(
            waitForFrontmatter(id: Fixtures.Ids.unicode) { $0.tags.contains("unicode") },
            "the card's original tag is untouched"
        )
        reader.close()

        XCTAssertTrue(list.waitForRowCount(Fixtures.standardCorpus.count), "full board returns")
        XCTAssertTrue(list.row(Fixtures.Ids.minimal).waitExists(), "minimal is visible again")
        XCTAssertTrue(list.row(Fixtures.Ids.unicode).waitExists(), "unicode is visible again")
    }

    private func openReading(_ id: String) {
        list.select(id)
        keyboard.openWithReturn()
    }
}
