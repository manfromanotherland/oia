// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import XCTest

final class InspectorJourney: UITestCase {
    func testSidebarLayoutAndCollapse() throws {
        try launchApp(articles: Fixtures.standardCorpus)
        list.open(Fixtures.Ids.minimal)

        let inspector = app.byId(A11y.Inspector.panel)
        let toggle = app.byId(A11y.Inspector.toggle)
        let close = app.byId(A11y.Detail.close)
        XCTAssertTrue(inspector.waitExists())

        let windowMidX = app.windows.firstMatch.frame.midX
        XCTAssertLessThan(inspector.frame.midX, windowMidX, "Inspector is on the left")
        XCTAssertLessThan(toggle.frame.midX, windowMidX, "Inspector toggle is on the left")
        XCTAssertGreaterThanOrEqual(close.frame.minX, inspector.frame.maxX, "Close sits outside the inspector")
        XCTAssertLessThan(toggle.frame.midX, close.frame.midX, "Close follows the sidebar toggle")
        XCTAssertLessThan(close.frame.midX, app.byId(A11y.Detail.previous).frame.midX, "Navigation follows Close")
        XCTAssertLessThan(close.frame.midX, windowMidX, "Close is on the left")
        let expandedCloseX = close.frame.midX

        toggle.click()
        XCTAssertTrue(inspector.waitDisappears(), "Inspector collapses")
        let controlsFollowDetail = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in close.frame.midX < expandedCloseX - 100 },
            object: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [controlsFollowDetail], timeout: 2), .completed,
            "Close follows the collapsing detail pane"
        )
        toggle.click()
        XCTAssertTrue(inspector.waitExists(), "Toolbar button expands the sidebar")
        keyboard.toggleSidebar()
        XCTAssertTrue(inspector.waitDisappears(), "⌘B collapses the sidebar")
        keyboard.toggleSidebar()
        XCTAssertTrue(inspector.waitExists(), "⌘B expands the sidebar")
        keyboard.escape()
        XCTAssertTrue(app.byId(A11y.List.rows).waitExists(), "Escape closes the detail view")
    }

    func testFileFactsTagsAndColourSearch() throws {
        let cream = try seedImages()
        relaunchApp { $0.pinnedDefaults = ["appearanceMode": "dark", "showsReadingInspector": "1"] }
        XCTAssertTrue(list.waitForRowCount(2))
        list.open(cream)
        XCTAssertTrue(app.byId(A11y.Inspector.tabs).waitExists())
        XCTAssertTrue(app.staticTexts["Your tags"].exists)
        XCTAssertFalse(app.staticTexts.matching(
            NSPredicate(format: "label CONTAINS 'cuttings-asset:'")
        ).firstMatch.exists)

        selectInspectorTab("Details")
        XCTAssertTrue(app.staticTexts["PNG"].waitExists())
        XCTAssertTrue(app.staticTexts["64 × 64 px"].exists)
        capture("Sidebar inspector · Details")

        selectInspectorTab("Discover")
        app.byId(A11y.Inspector.editTags).clickWhenReady()
        XCTAssertTrue(app.byId(A11y.TagPicker.done).waitExists())
        app.byId(A11y.TagPicker.done).clickWhenReady()
        let swatch = app.buttons.matching(
            NSPredicate(format: "identifier BEGINSWITH %@", A11y.Inspector.colorPrefix)
        ).firstMatch
        XCTAssertTrue(swatch.waitForExistence(timeout: 45), "Cached analysis publishes clickable colours")
        capture("Sidebar inspector · Discover")
        swatch.click()
        XCTAssertTrue(list.waitForRowCount(1), "Colour search excludes the blue image")
        XCTAssertEqual(list.orderedRowIds, [cream])
        XCTAssertFalse(app.byId(A11y.Inspector.tabs).exists, "Searching returns to the board")
    }

    private func seedImages() throws -> String {
        try launchApp()
        app.terminate()
        let cream = String(repeating: "a", count: 64)
        try writeImage(id: cream, title: "Warm room", color: NSColor(srgbRed: 0.74, green: 0.66, blue: 0.56, alpha: 1))
        try writeImage(
            id: String(repeating: "b", count: 64), title: "Blue study",
            color: NSColor(srgbRed: 0.1, green: 0.2, blue: 0.9, alpha: 1)
        )
        return cream
    }

    private func selectInspectorTab(_ label: String) {
        let tabs = app.byId(A11y.Inspector.tabs)
        let radio = tabs.radioButtons[label]
        (radio.exists ? radio : tabs.buttons[label]).clickWhenReady()
    }

    private func capture(_ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    private func writeImage(id: String, title: String, color: NSColor) throws {
        let data = try InspectorAnalysisFixture.png(color: color)
        let fixture = ArticleFixture(
            id: id, url: "https://example.com/\(id)", title: title, savedAt: Date(), tags: ["Interiors"]
        )
        let markdown = fixture.rendered().replacingOccurrences(
            of: "format_version: 1",
            with: """
            format_version: 1
            kind: image
            media_url: cuttings-asset:assets/image.png
            preview_asset: assets/image.png
            """
        )
        try library.writeRaw(id: id, contents: markdown)
        try library.writeAsset(articleId: id, fileName: "image.png", data: data)
        try InspectorAnalysisFixture.seed(dbURL: library.dbURL, image: data, color: color)
    }
}
