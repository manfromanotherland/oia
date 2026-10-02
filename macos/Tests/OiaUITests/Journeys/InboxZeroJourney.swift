// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// Inbox zero: the core triage loop in the Unread view. Archive (⌘⌫), mark
/// read (⌘U), favorite (⌘⇧F), and add a tag, asserting the one-motion row removal
/// + selection advance, live sidebar counts, the empty state, and on-disk
/// frontmatter after each mutation.
///
/// The Unread view (default sort, saved-at descending) lists the five unread
/// articles in this order — each action operates on the current selection, and
/// removals advance the selection to the next row *without* re-clicking:
///   swiftTips(7) → minimal(4) → unicode(3) → kitchenSink(2) → swift(1)
final class InboxZeroJourney: UITestCase {
    // A single end-to-end triage walk; asserting every beat keeps it over the limit.
    // swiftlint:disable:next function_body_length
    func testTriageUnreadToZero() throws {
        // The fixed newest-saved-first order is [7,4,3,2,1].
        try launchApp(articles: Fixtures.standardCorpus)
        sidebar.select(.unread)
        XCTAssertTrue(list.waitForRowCount(5), "Unread starts with 5")

        // Select the first unread article.
        list.open(Fixtures.Ids.swiftTips)
        XCTAssertEqual(reader.titleText, "Swift Tips")

        // 1. Not worth it → Archive (⌘⌫): row leaves Unread, selection advances to
        //    the next article, Unread 5→4, Archive 2→3, file archived.
        keyboard.archive()
        XCTAssertTrue(list.row(Fixtures.Ids.swiftTips).waitDisappears(), "archived row leaves Unread")
        XCTAssertTrue(sidebar.waitForCount(.unread, equals: 4), "Unread 5→4")
        XCTAssertTrue(sidebar.waitForCount(.archive, equals: 3), "Archive 2→3")
        XCTAssertEqual(reader.titleText, "Minimal", "selection advanced to the next row")
        XCTAssertTrue(waitForFrontmatter(id: Fixtures.Ids.swiftTips) { $0.archived }, "archived on disk")

        // 2. Worth reading → Mark Read (⌘U): leaves Unread, Unread 4→3, Read 3→4,
        //    selection advances, file gets read_at.
        keyboard.markRead()
        XCTAssertTrue(list.row(Fixtures.Ids.minimal).waitDisappears(), "read row leaves Unread")
        XCTAssertTrue(sidebar.waitForCount(.unread, equals: 3), "Unread 4→3")
        XCTAssertTrue(sidebar.waitForCount(.read, equals: 4), "Read 3→4")
        XCTAssertEqual(reader.titleText, "Café Über 日本語 🎉", "selection advanced")
        XCTAssertTrue(waitForFrontmatter(id: Fixtures.Ids.minimal) { $0.isRead }, "read on disk")

        // 3. A keeper → Favorite (⌘⇧F): STAYS in Unread (favoriting doesn't change
        //    read state), Favorites 3→4, a heart appears on the row, file favorited.
        keyboard.toggleFavorite()
        XCTAssertTrue(sidebar.waitForCount(.favorites, equals: 4), "Favorites 3→4")
        XCTAssertTrue(sidebar.waitForCount(.unread, equals: 3), "still Unread 3 (favorite doesn't remove)")
        XCTAssertTrue(list.row(Fixtures.Ids.unicode).waitExists(), "still present in Unread")
        XCTAssertTrue(list.rowHasIndicator(Fixtures.Ids.unicode, label: "Favorite"), "heart on the row")
        XCTAssertTrue(waitForFrontmatter(id: Fixtures.Ids.unicode) { $0.favorite }, "favorite on disk")

        // 4. Also read later → add the "weekend" tag in the Inspector. Chip on the
        //    article, a new sidebar tile with count 1, tag written to the file.
        reader.focusTagInput()
        XCTAssertTrue(tagInput.isVisible, "Inspector Tag input opened")
        tagInput.add("weekend")
        XCTAssertTrue(sidebar.waitForTagCount("weekend", equals: 1), "sidebar gains #weekend (1)")
        XCTAssertTrue(waitForFrontmatter(id: Fixtures.Ids.unicode) { $0.tags.contains("weekend") }, "tag on disk")

        // 5. Done with it → Mark Read (⌘U): leaves Unread, Unread 3→2, selection
        //    advances to the kitchen-sink article.
        keyboard.markRead()
        XCTAssertTrue(list.row(Fixtures.Ids.unicode).waitDisappears(), "read row leaves Unread")
        XCTAssertTrue(sidebar.waitForCount(.unread, equals: 2), "Unread 3→2")
        XCTAssertEqual(reader.titleText, "The Complete Markdown Sample", "selection advanced")

        // 6. Junk → Archive (⌘⌫): Unread 2→1, Archive 3→4, selection advances.
        keyboard.archive()
        XCTAssertTrue(list.row(Fixtures.Ids.kitchenSink).waitDisappears(), "archived row leaves Unread")
        XCTAssertTrue(sidebar.waitForCount(.unread, equals: 1), "Unread 2→1")
        XCTAssertTrue(sidebar.waitForCount(.archive, equals: 4), "Archive 3→4")
        XCTAssertEqual(reader.titleText, "Swift Concurrency Explained", "selection advanced")

        // 7. Last one → Mark Read (⌘U): Unread → 0, and the list shows the empty state.
        keyboard.markRead()
        XCTAssertTrue(sidebar.waitForCount(.unread, equals: 0), "Unread → 0")
        XCTAssertTrue(list.emptyState.waitExists(), "empty state appears")
    }
}
