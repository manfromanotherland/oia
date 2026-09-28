// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class GalleryNavigationJourney: UITestCase {
    func testNavigateGalleryWithKeyboard() throws {
        try launchApp(articles: Fixtures.standardCorpus)

        list.open(Fixtures.Ids.minimal)
        XCTAssertTrue(app.byId(A11y.Detail.next).waitExists(), "Gallery detail opens")
        assertGalleryShows("Minimal")

        keyboard.nextItem()
        assertGalleryShows("Café Über 日本語 🎉")

        keyboard.previousItem()
        assertGalleryShows("Minimal")

        keyboard.arrowRight()
        assertGalleryShows("Café Über 日本語 🎉")

        keyboard.arrowLeft()
        assertGalleryShows("Minimal")
    }

    private func assertGalleryShows(
        _ title: String,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        XCTAssertTrue(
            wait { reader.titleText == title },
            "Gallery moved to \(title)",
            file: file,
            line: line
        )
    }
}
