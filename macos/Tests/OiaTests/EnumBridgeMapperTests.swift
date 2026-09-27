// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// The enum bridges (`Bridge/Mappers`) map the app-language library scope and
/// sort field onto the core's query enums. These tests pin every retained case, so a reordering
/// or rename on either side is caught. The `switch`es are exhaustive, so a new
/// enum case is a compile error until it is mapped.
final class EnumBridgeMapperTests: XCTestCase {
    func testLibraryScopeMapsEveryViewCase() {
        XCTAssertEqual(LibraryScope.all.ffiView, .all)
        XCTAssertEqual(LibraryScope.images.ffiView, .all)
        XCTAssertEqual(LibraryScope.videos.ffiView, .all)
        XCTAssertEqual(LibraryScope.articles.ffiView, .articles)
        XCTAssertEqual(LibraryScope.links.ffiView, .links)
        XCTAssertEqual(LibraryScope.quotes.ffiView, .quotes)
    }

    func testLibraryScopesUseToolbarOrderAndLabels() {
        XCTAssertEqual(
            LibraryScope.allCases.map(\.label),
            ["All", "Images", "Videos", "Articles", "Links", "Quotes"]
        )
    }

    func testLibraryScopesUseRequestedToolbarIcons() {
        XCTAssertEqual(
            LibraryScope.allCases.map(\.icon),
            [
                "asterisk", "photo", "play.rectangle", "newspaper", "link", "quote.opening"
            ]
        )
    }

    func testLibraryScopeNavigationWrapsInToolbarOrder() {
        XCTAssertEqual(LibraryScope.all.previous, .quotes)
        XCTAssertEqual(LibraryScope.all.next, .images)
        XCTAssertEqual(LibraryScope.images.next, .videos)
        XCTAssertEqual(LibraryScope.videos.next, .articles)
        XCTAssertEqual(LibraryScope.articles.previous, .videos)
        XCTAssertEqual(LibraryScope.articles.next, .links)
        XCTAssertEqual(LibraryScope.quotes.next, .all)
    }

    func testReadingSortMapsEverySortCase() {
        XCTAssertEqual(ReadingSort.relevance.ffiSort, .relevance)
        XCTAssertEqual(ReadingSort.savedAt.ffiSort, .savedAt)
    }
}
