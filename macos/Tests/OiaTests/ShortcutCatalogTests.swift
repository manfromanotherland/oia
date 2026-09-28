// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import XCTest

final class ShortcutCatalogTests: XCTestCase {
    func testPrimaryActionShortcutsFollowMacConventions() {
        XCTAssertTrue(ShortcutCatalog.copy.matches(key: "c", modifiers: .command))
        XCTAssertTrue(ShortcutCatalog.copyAddress.matches(key: "c", modifiers: [.command, .shift]))
        XCTAssertTrue(ShortcutCatalog.open.matches(key: "o", modifiers: .command))
        XCTAssertTrue(ShortcutCatalog.open.matches(key: .return, modifiers: []))
        XCTAssertTrue(ShortcutCatalog.delete.matches(key: .delete, modifiers: .command))
        XCTAssertFalse(
            ShortcutCatalog.delete.matches(
                key: .delete,
                modifiers: [.command, .option]
            )
        )
    }

    func testSearchSupportsCommandFAndBoardLocalSlash() {
        XCTAssertTrue(ShortcutCatalog.focusSearch.matches(key: "f", modifiers: .command))
        XCTAssertTrue(ShortcutCatalog.focusSearch.matches(key: "/", modifiers: []))
        XCTAssertFalse(ShortcutCatalog.focusSearch.matches(key: "/", modifiers: .command))
        XCTAssertEqual(ShortcutCatalog.focusSearch.displays, ["⌘F", "/"])
    }

    func testQuickLookIsBoardLocalSpace() {
        XCTAssertTrue(ShortcutCatalog.quickLook.matches(key: .space, modifiers: []))
        XCTAssertFalse(ShortcutCatalog.quickLook.matches(key: .space, modifiers: .command))
        XCTAssertEqual(ShortcutCatalog.quickLook.display, "Space")
    }

    func testSidebarUsesCommandB() {
        XCTAssertTrue(ShortcutCatalog.toggleSidebar.matches(key: "b", modifiers: .command))
        XCTAssertEqual(ShortcutCatalog.toggleSidebar.display, "⌘B")
    }

    func testEveryScopeHasItsOrderedCommandNumber() {
        XCTAssertEqual(
            LibraryScope.allCases.map(\.label),
            ["All", "Images", "Videos", "Articles", "Links", "Quotes"]
        )
        XCTAssertEqual(
            LibraryScope.allCases.map { ShortcutCatalog.filterShortcut(for: $0).display },
            ["⌘1", "⌘2", "⌘3", "⌘4", "⌘5", "⌘6"]
        )
        XCTAssertEqual(ShortcutCatalog.previousFilter.display, "⌘[")
        XCTAssertEqual(ShortcutCatalog.nextFilter.display, "⌘]")
    }
}
