// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI
import XCTest

@MainActor
final class BoardScopePickerTests: XCTestCase {
    func testKeepsSwiftUISegmentedRendererAndCell() throws {
        let reference = NSHostingView(rootView:
            Picker("Filter", selection: .constant(LibraryScope.all)) {
                ForEach(LibraryScope.allCases) { scope in
                    Text(scope.label).tag(scope)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        )
        let actual = NSHostingView(rootView: picker(Selection(), count: 12))
        for host in [reference as NSView, actual as NSView] {
            host.frame = CGRect(x: 0, y: 0, width: 800, height: 50)
            host.layoutSubtreeIfNeeded()
        }
        let native = try XCTUnwrap(segmentedControl(in: reference))
        let configured = try XCTUnwrap(segmentedControl(in: actual))
        XCTAssertEqual(NSStringFromClass(type(of: configured)), NSStringFromClass(type(of: native)))
        XCTAssertEqual(
            configured.cell.map { NSStringFromClass(type(of: $0)) },
            native.cell.map { NSStringFromClass(type(of: $0)) }
        )
    }

    private func segmentedControl(in view: NSView) -> NSSegmentedControl? {
        if let control = view as? NSSegmentedControl { return control }
        return view.subviews.lazy.compactMap { self.segmentedControl(in: $0) }.first
    }

    func testAttachesImageMenuIndicatorWithoutReplacingNativeControlOrCell() throws {
        let state = Selection()
        let coordinator = picker(state, count: 12).makeCoordinator()
        let action = ActionSpy()
        let control = nativeControl(target: action)
        let cell = try XCTUnwrap(control.cell)
        let height = control.intrinsicContentSize.height
        coordinator.attach(to: control)

        XCTAssertEqual(control.segmentCount, 6)
        XCTAssertTrue(control.cell === cell)
        XCTAssertEqual(control.segmentStyle, .rounded)
        XCTAssertEqual(control.segmentDistribution, .fillProportionally)
        XCTAssertEqual(control.trackingMode, .selectOne)
        XCTAssertEqual(control.selectedSegment, 4)
        XCTAssertEqual(control.intrinsicContentSize.height, height)
        for (index, scope) in LibraryScope.allCases.enumerated() {
            XCTAssertEqual(control.label(forSegment: index), scope.label)
            XCTAssertNil(control.menu(forSegment: index), "The Picker keeps its native selection tracking")
            XCTAssertEqual(control.showsMenuIndicator(forSegment: index), scope == .images)
        }
        let menu = coordinator.imageMenu
        XCTAssertEqual(menu.items.map(\.title), ["All Images", "Graphics", "Photos", "Unsure"])
        XCTAssertEqual(menu.items.last?.badge?.itemCount, 12)
        XCTAssertEqual(menu.items.last?.accessibilityLabel(), "Unsure, 12 images to classify")
        XCTAssertTrue(try XCTUnwrap(menu.items.last).isEnabled)
        XCTAssertEqual(action.callCount, 0)
        XCTAssertTrue(state.writes.isEmpty)
    }

    func testUnsureRemainsVisibleButDisabledAtZero() throws {
        let state = Selection()
        let coordinator = picker(state, count: 12).makeCoordinator()
        let action = ActionSpy()
        let control = nativeControl(target: action)
        coordinator.attach(to: control)
        coordinator.parent = picker(state, count: 0)
        coordinator.updateMenu()

        let item = try XCTUnwrap(coordinator.imageMenu.items.last)
        XCTAssertEqual(coordinator.imageMenu.items.count, 4)
        XCTAssertEqual(item.title, "Unsure")
        XCTAssertEqual(item.badge?.itemCount, 0)
        XCTAssertEqual(item.accessibilityLabel(), "Unsure, 0 images to classify")
        XCTAssertFalse(item.isEnabled)
        coordinator.selectImageType(item)
        XCTAssertEqual(state.scope, .all)
        XCTAssertEqual(state.imageType, .photos)
        XCTAssertTrue(state.writes.isEmpty)
    }

    func testNativeActionRunsBeforeScopeBindingChangesAndRepeatsForImages() {
        let state = Selection()
        let coordinator = picker(state, count: 1).makeCoordinator()
        let action = ActionSpy()
        let control = nativeControl(target: action)
        coordinator.attach(to: control)
        action.onSelection = { sender in
            XCTAssertEqual(state.scope, .all, "The original action receives the selection before any binding write")
            state.scope = LibraryScope.allCases[sender.selectedSegment]
        }

        control.selectedSegment = 1
        coordinator.selectionChanged(control)
        XCTAssertEqual(action.callCount, 1)
        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(state.imageType, .photos)
        XCTAssertTrue(state.writes.isEmpty, "The native Picker action owns the scope binding")

        action.onSelection = { sender in
            state.scope = LibraryScope.allCases[sender.selectedSegment]
        }
        coordinator.selectionChanged(control)
        XCTAssertEqual(action.callCount, 2, "Pressing the selected Images segment forwards the native action again")
        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(state.imageType, .photos)
        XCTAssertEqual(control.selectedSegment, 1)
    }

    func testMenuSelectionAndScopeChangesPreserveTheRememberedFacet() {
        let state = Selection()
        let coordinator = picker(state, count: 1).makeCoordinator()
        let action = ActionSpy()
        let control = nativeControl(target: action)
        action.onSelection = { sender in
            state.scope = LibraryScope.allCases[sender.selectedSegment]
        }
        coordinator.attach(to: control)

        control.selectedSegment = 1
        coordinator.selectionChanged(control)

        let graphics = coordinator.imageMenu.items[1]
        coordinator.selectImageType(graphics)
        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(state.imageType, .graphics)
        XCTAssertEqual(graphics.state, .on)

        control.selectedSegment = 3
        coordinator.selectionChanged(control)
        XCTAssertEqual(state.scope, .articles)
        XCTAssertEqual(state.imageType, .graphics)

        control.selectedSegment = 1
        coordinator.selectionChanged(control)
        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(graphics.state, .on)
        coordinator.selectImageType(coordinator.imageMenu.items[0])
        XCTAssertEqual(state.imageType, .all)
    }

    func testMenuSelectionLeavesNativeSelectionToSwiftUI() {
        let state = Selection()
        state.scope = .articles
        let coordinator = picker(state, count: 1).makeCoordinator()
        let control = nativeControl(target: ActionSpy())
        control.selectedSegment = 3
        coordinator.attach(to: control)

        coordinator.selectImageType(coordinator.imageMenu.items[1])

        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(state.imageType, .graphics)
        XCTAssertEqual(state.writes, ["scope", "imageType"])
        XCTAssertEqual(control.selectedSegment, 3, "SwiftUI reconciles its own selected segment from the binding")
    }

    func testProgrammaticUpdatesDoNotSendActionsWriteBindingsOrOpenMenu() throws {
        let state = Selection()
        let coordinator = picker(state, count: 12).makeCoordinator()
        let action = ActionSpy()
        let control = nativeControl(target: action)
        let cell = try XCTUnwrap(control.cell)
        coordinator.attach(to: control)
        let menuObserver = MenuOpenSpy()
        coordinator.imageMenu.delegate = menuObserver

        state.scope = .images
        state.imageType = .graphics
        control.selectedSegment = 1
        coordinator.parent = picker(state, count: 0)
        coordinator.updateMenu()

        XCTAssertEqual(action.callCount, 0)
        XCTAssertEqual(menuObserver.openCount, 0)
        XCTAssertTrue(state.writes.isEmpty)
        XCTAssertEqual(state.scope, .images)
        XCTAssertEqual(state.imageType, .graphics)
        XCTAssertEqual(control.selectedSegment, 1)
        XCTAssertTrue(control.cell === cell)
        XCTAssertEqual(control.segmentStyle, .rounded)
        XCTAssertEqual(control.segmentDistribution, .fillProportionally)
        XCTAssertEqual(control.trackingMode, .selectOne)
        XCTAssertEqual(coordinator.imageMenu.items[1].state, .on)
        XCTAssertEqual(coordinator.imageMenu.items.last?.badge?.itemCount, 0)
    }

    func testDetachRestoresOriginalTargetActionAndMenuIndicatorAfterRepeatedAttachment() {
        for originalIndicator in [false, true] {
            let coordinator = picker(Selection(), count: 12).makeCoordinator()
            let action = ActionSpy()
            let control = nativeControl(target: action)
            control.setShowsMenuIndicator(originalIndicator, forSegment: 1)
            let originalAction = control.action

            coordinator.attach(to: control)
            coordinator.attach(to: control)
            coordinator.detach()

            XCTAssertTrue(control.target as AnyObject? === action)
            XCTAssertEqual(control.action, originalAction)
            XCTAssertEqual(control.showsMenuIndicator(forSegment: 1), originalIndicator)
            XCTAssertEqual(control.selectedSegment, 4)
            XCTAssertEqual(action.callCount, 0)
        }
    }

    private final class Selection {
        var scope: LibraryScope = .all
        var imageType: ImageTypeFilter = .photos
        var writes: [String] = []
    }

    private final class ActionSpy: NSObject {
        var callCount = 0
        var onSelection: ((NSSegmentedControl) -> Void)?

        @objc func selectionChanged(_ sender: NSSegmentedControl) {
            callCount += 1
            onSelection?(sender)
        }
    }

    private final class MenuOpenSpy: NSObject, NSMenuDelegate {
        var openCount = 0

        func menuWillOpen(_: NSMenu) {
            openCount += 1
        }
    }

    private func nativeControl(target: ActionSpy) -> NSSegmentedControl {
        let control = NSSegmentedControl(
            labels: LibraryScope.allCases.map(\.label), trackingMode: .selectOne,
            target: target, action: #selector(ActionSpy.selectionChanged(_:))
        )
        control.segmentStyle = .rounded
        control.segmentDistribution = .fillProportionally
        control.selectedSegment = 4
        return control
    }

    private func picker(_ state: Selection, count: Int) -> BoardScopePicker {
        BoardScopePicker(
            scope: Binding(get: { state.scope }, set: {
                state.writes.append("scope")
                state.scope = $0
            }),
            imageType: Binding(get: { state.imageType }, set: {
                state.writes.append("imageType")
                state.imageType = $0
            }),
            unsureImageCount: count
        )
    }
}
