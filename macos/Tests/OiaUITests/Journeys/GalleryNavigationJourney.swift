// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class GalleryNavigationJourney: UITestCase {
    func testNavigateReadingsWithKeyboard() throws {
        try launchApp(articles: Fixtures.standardCorpus)

        list.open(Fixtures.Ids.minimal)
        XCTAssertTrue(app.byId(A11y.Detail.next).waitExists(), "Reading detail opens")
        assertReadingShows("Minimal")

        keyboard.nextItem()
        assertReadingShows("Café Über 日本語 🎉")

        keyboard.previousItem()
        assertReadingShows("Minimal")

        keyboard.arrowRight()
        assertReadingShows("Café Über 日本語 🎉")

        keyboard.arrowLeft()
        assertReadingShows("Minimal")
    }

    private func assertReadingShows(
        _ title: String,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        XCTAssertTrue(
            wait { reader.titleText == title },
            "Reading detail moved to \(title)",
            file: file,
            line: line
        )
    }
}
