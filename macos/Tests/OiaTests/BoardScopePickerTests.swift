// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI
import XCTest

@MainActor
final class BoardScopePickerTests: XCTestCase {
    func testOnlyImagesHasAMenuWithoutChangingNativeHeight() throws {
        let state = Selection()
        let coordinator = picker(state, count: 12).makeCoordinator()
        let control = coordinator.makeControl()
        let reference = NSSegmentedControl(
            labels: LibraryScope.allCases.map(\.label), trackingMode: .selectOne,
            target: nil, action: nil
        )
        reference.segmentDistribution = .fillEqually

        XCTAssertEqual(control.segmentCount, 6)
        XCTAssertEqual(control.intrinsicContentSize.height, reference.intrinsicContentSize.height)
        for (index, scope) in LibraryScope.allCases.enumerated() {
            XCTAssertEqual(control.label(forSegment: index), scope.label)
            XCTAssertEqual(control.menu(forSegment: index) != nil, scope == .images)
            XCTAssertEqual(control.showsMenuIndicator(forSegment: index), scope == .images)
        }
        let menu = try XCTUnwrap(control.menu(forSegment: 1))
        XCTAssertEqual(menu.items.map(\.title), ["All Images", "Graphics", "Photos", "Unsure"])
        XCTAssertEqual(menu.items.last?.badge?.itemCount, 12)
        XCTAssertTrue(try XCTUnwrap(menu.items.last).isEnabled)
    }

    func testUnsureRemainsVisibleButDisabledAtZero() throws {
        let state = Selection()
        let coordinator = picker(state, count: 12).makeCoordinator()
        let control = coordinator.makeControl()
        coordinator.parent = picker(state, count: 0)
        coordinator.update(control)

        let item = try XCTUnwrap(coordinator.imageMenu.items.last)
        XCTAssertEqual(item.title, "Unsure")
        XCTAssertEqual(item.badge?.itemCount, 0)
        XCTAssertEqual(item.accessibilityLabel(), "Unsure, 0 images to classify")
        XCTAssertFalse(item.isEnabled)
        coordinator.selectImageType(item)
        XCTAssertEqual(state.scope, .all)
        XCTAssertEqual(state.imageType, .photos)
    }

    func testMenuSelectionAndScopeChangesPreserveTheRememberedFacet() throws {
        let state = Selection()
        let coordinator = picker(state, count: 1).makeCoordinator()
        let control = coordinator.makeControl()

        control.selectedSegment = 1
        coordinator.selectScope(control)
        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(state.imageType, .photos)

        let graphics = coordinator.imageMenu.items[1]
        coordinator.selectImageType(graphics)
        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(state.imageType, .graphics)
        XCTAssertEqual(graphics.state, .on)

        control.selectedSegment = 3
        coordinator.selectScope(control)
        XCTAssertEqual(state.scope, .articles)
        XCTAssertEqual(state.imageType, .graphics)

        // AppKit opens the attached menu itself for press-and-hold or VoiceOver.
        coordinator.menuWillOpen(coordinator.imageMenu)
        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(graphics.state, .on)
        coordinator.selectImageType(coordinator.imageMenu.items[0])
        XCTAssertEqual(state.imageType, .all)
    }

    private final class Selection {
        var scope: LibraryScope = .all
        var imageType: ImageTypeFilter = .photos
    }

    private func picker(_ state: Selection, count: Int) -> BoardScopePicker {
        BoardScopePicker(
            scope: Binding(get: { state.scope }, set: { state.scope = $0 }),
            imageType: Binding(get: { state.imageType }, set: { state.imageType = $0 }),
            unsureImageCount: count
        )
    }
}
