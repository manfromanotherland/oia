// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// `LibraryScope.contains` mirrors the board scopes exposed by the app. Legacy
/// read/archive/favorite metadata must not hide a card now that those states
/// are no longer part of the organizing model.
final class LibraryScopeContainsTests: XCTestCase {
    func testAllContainsEveryReadingIncludingLegacyArchivedRows() {
        XCTAssertTrue(LibraryScope.all.contains(makeReadingRow()))
        XCTAssertTrue(LibraryScope.all.contains(makeReadingRow(read: true)))
        XCTAssertTrue(LibraryScope.all.contains(makeReadingRow(archived: true)))
        XCTAssertTrue(
            LibraryScope.all.contains(makeReadingRow(read: true, archived: true, favorite: true))
        )
    }

    func testImagesAndVideosAreDistinctScopes() {
        let image = makeReadingRow(kind: .image)
        let video = makeReadingRow(kind: .video)
        XCTAssertTrue(LibraryScope.images.contains(image))
        XCTAssertFalse(LibraryScope.images.contains(video))
        XCTAssertTrue(LibraryScope.videos.contains(video))
        XCTAssertFalse(LibraryScope.videos.contains(image))
        XCTAssertFalse(LibraryScope.images.contains(makeReadingRow(kind: .article)))
        XCTAssertFalse(LibraryScope.videos.contains(makeReadingRow(kind: .quote)))
    }

    func testArticlesExcludeLinksIncludingFullSocialPosts() {
        XCTAssertTrue(LibraryScope.articles.contains(makeReadingRow(kind: .article)))
        XCTAssertFalse(
            LibraryScope.articles.contains(makeReadingRow(lightweight: true, isLink: true))
        )
        XCTAssertFalse(LibraryScope.articles.contains(makeReadingRow(isLink: true)))
        XCTAssertFalse(LibraryScope.articles.contains(makeReadingRow(kind: .image)))
    }

    func testLinksContainLightweightLinksAndFullSocialPosts() {
        XCTAssertTrue(
            LibraryScope.links.contains(makeReadingRow(lightweight: true, isLink: true))
        )
        XCTAssertTrue(LibraryScope.links.contains(makeReadingRow(isLink: true)))
        XCTAssertFalse(LibraryScope.links.contains(makeReadingRow(kind: .article)))
        XCTAssertFalse(
            LibraryScope.links.contains(makeReadingRow(lightweight: true, isLink: true, kind: .video))
        )
    }

    func testQuotesContainsQuoteReadingsOnly() {
        XCTAssertTrue(LibraryScope.quotes.contains(makeReadingRow(kind: .quote)))
        XCTAssertFalse(LibraryScope.quotes.contains(makeReadingRow(kind: .article)))
    }
}
