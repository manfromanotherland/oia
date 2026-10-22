// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import XCTest

final class ShortcutCatalogTests: XCTestCase {
    func testPrimaryActionShortcutsFollowMacConventions() {
        XCTAssertTrue(ShortcutCatalog.copy.matches(key: "c", modifiers: .command))
        XCTAssertTrue(ShortcutCatalog.openSource.matches(key: "o", modifiers: .command))
        XCTAssertFalse(ShortcutCatalog.openSource.matches(key: "o", modifiers: [.command, .shift]))
        XCTAssertTrue(ShortcutCatalog.copySourceURL.matches(key: "c", modifiers: [.command, .shift]))
        XCTAssertFalse(ShortcutCatalog.copySourceURL.matches(key: "c", modifiers: .command))
        XCTAssertTrue(ShortcutCatalog.editTags.matches(key: "t", modifiers: .command))
        XCTAssertFalse(ShortcutCatalog.editTags.matches(key: "t", modifiers: [.command, .shift]))
        XCTAssertEqual(ShortcutCatalog.copySourceURL.display, "⇧⌘C")
    }

    func testOpenDetailUsesOnlyUnmodifiedReturn() {
        XCTAssertTrue(ShortcutCatalog.open.matches(key: .return, modifiers: []))
        XCTAssertFalse(ShortcutCatalog.open.matches(key: "o", modifiers: .command))
        XCTAssertFalse(ShortcutCatalog.open.matches(key: .return, modifiers: .command))
        XCTAssertEqual(ShortcutCatalog.open.displays, ["↩"])
        XCTAssertEqual(ShortcutCatalog.open.title, "Open Detail")
    }

    func testMarkAsPhotoHasAnUnambiguousShortcutInBothContexts() throws {
        XCTAssertTrue(ShortcutCatalog.markAsPhoto.matches(key: "p", modifiers: [.command, .shift]))
        XCTAssertFalse(ShortcutCatalog.markAsPhoto.matches(key: "p", modifiers: .command))
        XCTAssertFalse(ShortcutCatalog.markAsPhoto.matches(key: "p", modifiers: []))
        XCTAssertEqual(ShortcutCatalog.markAsPhoto.display, "⇧⌘P")
        XCTAssertEqual(ShortcutCatalog.markAsPhoto.title, "Mark as Photo")

        let shared = try XCTUnwrap(ShortcutCatalog.groups.first { $0.name == "Grid & Detail" })
        XCTAssertTrue(shared.shortcuts.contains { $0.id == ShortcutCatalog.markAsPhoto.id })
        let matchingActions = ShortcutCatalog.groups.flatMap(\.shortcuts).filter {
            $0.matches(key: "p", modifiers: [.command, .shift])
        }
        XCTAssertEqual(matchingActions.map(\.id), [ShortcutCatalog.markAsPhoto.id])
    }

    func testMarkAsGraphicHasAnUnambiguousShortcutInBothContexts() throws {
        XCTAssertTrue(ShortcutCatalog.markAsGraphic.matches(key: "g", modifiers: [.command, .shift]))
        XCTAssertFalse(ShortcutCatalog.markAsGraphic.matches(key: "g", modifiers: .command))
        XCTAssertFalse(ShortcutCatalog.markAsGraphic.matches(key: "g", modifiers: []))
        XCTAssertEqual(ShortcutCatalog.markAsGraphic.display, "⇧⌘G")
        XCTAssertEqual(ShortcutCatalog.markAsGraphic.title, "Mark as Graphic")

        let shared = try XCTUnwrap(ShortcutCatalog.groups.first { $0.name == "Grid & Detail" })
        XCTAssertTrue(shared.shortcuts.contains { $0.id == ShortcutCatalog.markAsGraphic.id })
        let matchingActions = ShortcutCatalog.groups.flatMap(\.shortcuts).filter {
            $0.matches(key: "g", modifiers: [.command, .shift])
        }
        XCTAssertEqual(matchingActions.map(\.id), [ShortcutCatalog.markAsGraphic.id])
    }

    func testDeleteRequiresCommandWithoutOtherModifiers() {
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

    func testGridZoomUsesCommandMinusAndCommandPlusOrEquals() {
        XCTAssertTrue(ShortcutCatalog.decreaseCardSize.matches(key: "-", modifiers: .command))
        XCTAssertFalse(ShortcutCatalog.decreaseCardSize.matches(key: "-", modifiers: [.command, .shift]))
        XCTAssertTrue(ShortcutCatalog.increaseCardSize.matches(key: "+", modifiers: .command))
        XCTAssertTrue(ShortcutCatalog.increaseCardSize.matches(key: "=", modifiers: .command))
        XCTAssertFalse(ShortcutCatalog.increaseCardSize.matches(key: "=", modifiers: []))
        XCTAssertEqual(ShortcutCatalog.decreaseCardSize.displays, ["⌘-"])
        XCTAssertEqual(ShortcutCatalog.increaseCardSize.displays, ["⌘+", "⌘="])
    }

    func testDetailViewModesReuseTheFirstTwoGridFilterKeys() {
        XCTAssertTrue(ShortcutCatalog.selectFirstView.matches(key: "1", modifiers: .command))
        XCTAssertTrue(ShortcutCatalog.selectOriginalView.matches(key: "2", modifiers: .command))
        XCTAssertEqual(ShortcutCatalog.selectFirstView.display, ShortcutCatalog.showAll.display)
        XCTAssertEqual(ShortcutCatalog.selectOriginalView.display, ShortcutCatalog.showImages.display)
        XCTAssertFalse(ShortcutCatalog.selectFirstView.matches(key: "1", modifiers: []))
        XCTAssertFalse(ShortcutCatalog.selectOriginalView.matches(key: "2", modifiers: .option))
    }

    func testHelpGroupsExplainContextReusedBindings() throws {
        let grid = try XCTUnwrap(ShortcutCatalog.groups.first { $0.name == "Grid" })
        let detail = try XCTUnwrap(ShortcutCatalog.groups.first { $0.name == "Detail" })
        let text = try XCTUnwrap(ShortcutCatalog.groups.first { $0.name == "Saved Article Text" })
        let gridTitles = Set(grid.shortcuts.map(\.title))
        let detailTitles = Set(detail.shortcuts.map(\.title))

        XCTAssertTrue(gridTitles.contains(ShortcutCatalog.showAll.title))
        XCTAssertTrue(gridTitles.contains(ShortcutCatalog.showImages.title))
        XCTAssertTrue(gridTitles.contains(ShortcutCatalog.decreaseCardSize.title))
        XCTAssertTrue(gridTitles.contains(ShortcutCatalog.increaseCardSize.title))
        XCTAssertFalse(gridTitles.contains(ShortcutCatalog.selectFirstView.title))
        XCTAssertFalse(gridTitles.contains(ShortcutCatalog.selectOriginalView.title))
        XCTAssertTrue(detailTitles.contains(ShortcutCatalog.selectFirstView.title))
        XCTAssertTrue(detailTitles.contains(ShortcutCatalog.selectOriginalView.title))
        XCTAssertFalse(detailTitles.contains(ShortcutCatalog.showAll.title))
        XCTAssertFalse(detailTitles.contains(ShortcutCatalog.showImages.title))
        XCTAssertEqual(
            Set(text.shortcuts.map(\.title)),
            [ShortcutCatalog.increaseFont.title, ShortcutCatalog.decreaseFont.title]
        )
    }
}
