// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// SwiftUI owns the picker, including Liquid Glass and selection animation.
/// The background bridge adds only the Images menu to that existing control.
struct BoardScopePicker: View {
    @Binding var scope: LibraryScope
    @Binding var imageType: ImageTypeFilter
    let unsureImageCount: Int

    var body: some View {
        Picker("Filter", selection: $scope) {
            ForEach(LibraryScope.allCases) { scope in
                Text(scope.label)
                    .help(scope == .images ? "Show images and choose an image filter" : scope.label)
                    .accessibilityLabel(scope.label)
                    .tag(scope)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .accessibilityLabel("Filter")
        .accessibilityValue(scope.label)
        .accessibilityIdentifier(A11y.Filter.group)
        .background {
            MenuConfiguration(parent: self).frame(width: 0, height: 0)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    private struct MenuConfiguration: NSViewRepresentable {
        let parent: BoardScopePicker

        func makeCoordinator() -> Coordinator { parent.makeCoordinator() }

        func makeNSView(context: Context) -> MenuAnchorView {
            MenuAnchorView(coordinator: context.coordinator)
        }

        func updateNSView(_ view: MenuAnchorView, context: Context) {
            context.coordinator.parent = parent
            context.coordinator.updateMenu()
            view.configureWhenAttached()
        }

        static func dismantleNSView(_ view: MenuAnchorView, coordinator: Coordinator) {
            view.dismantle()
        }
    }

    @MainActor
    private final class MenuAnchorView: NSView {
        let coordinator: Coordinator
        private var isDismantled = false

        init(coordinator: Coordinator) {
            self.coordinator = coordinator
            super.init(frame: .zero)
            setAccessibilityElement(false)
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { nil }

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToSuperview() {
            super.viewDidMoveToSuperview()
            configureWhenAttached()
        }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if window == nil { coordinator.detach() }
            configureWhenAttached()
        }

        func configureWhenAttached() {
            configure()
            // SwiftUI can create or update its picker after the background view.
            DispatchQueue.main.async { [weak self] in self?.configure() }
        }

        func dismantle() {
            isDismantled = true
            coordinator.detach()
        }

        private func configure() {
            guard !isDismantled else { return }
            var ancestor = superview
            while let view = ancestor {
                if let control = scopePicker(in: view) {
                    coordinator.attach(to: control)
                    return
                }
                ancestor = view.superview
            }
        }

        private func scopePicker(in view: NSView) -> NSSegmentedControl? {
            if let control = view as? NSSegmentedControl,
               control.segmentCount == LibraryScope.allCases.count,
               LibraryScope.allCases.enumerated().allSatisfy({
                   control.label(forSegment: $0.offset) == $0.element.label
               }) {
                return control
            }
            return view.subviews.lazy.compactMap { self.scopePicker(in: $0) }.first
        }
    }

    @MainActor
    final class Coordinator: NSObject, NSMenuDelegate {
        var parent: BoardScopePicker
        let imageMenu = NSMenu(title: "Images")
        private weak var control: NSSegmentedControl?
        private weak var originalTarget: AnyObject?
        private var originalAction: Selector?
        private var originalMenuIndicator = false
        private var isMenuOpen = false
        private let imagesIndex = LibraryScope.allCases.firstIndex(of: .images)!

        init(_ parent: BoardScopePicker) {
            self.parent = parent
            super.init()
            imageMenu.autoenablesItems = false
            imageMenu.delegate = self
            imageMenu.identifier = NSUserInterfaceItemIdentifier(A11y.Filter.imageType)
            imageMenu.setAccessibilityIdentifier(A11y.Filter.imageType)
            for filter in ImageTypeFilter.allCases {
                let item = NSMenuItem(
                    title: filter == .all ? "All Images" : filter.label,
                    action: #selector(selectImageType(_:)), keyEquivalent: ""
                )
                item.target = self
                item.representedObject = filter.rawValue
                let identifier = A11y.Filter.imageType + "." + filter.rawValue
                item.identifier = NSUserInterfaceItemIdentifier(identifier)
                item.setAccessibilityIdentifier(identifier)
                imageMenu.addItem(item)
            }
            updateMenu()
        }

        func attach(to control: NSSegmentedControl) {
            if self.control !== control {
                detach()
                self.control = control
                originalMenuIndicator = control.showsMenuIndicator(forSegment: imagesIndex)
            }
            let action = #selector(selectionChanged(_:))
            if control.target !== self {
                originalTarget = control.target as AnyObject?
                originalAction = control.action
            } else if control.action != action {
                originalAction = control.action
            }
            control.target = self
            control.action = action
            if !control.showsMenuIndicator(forSegment: imagesIndex) {
                control.setShowsMenuIndicator(true, forSegment: imagesIndex)
            }
        }

        func detach() {
            if let control {
                if control.target === self {
                    control.target = originalTarget
                    control.action = originalAction
                }
                control.setShowsMenuIndicator(originalMenuIndicator, forSegment: imagesIndex)
            }
            control = nil
            originalTarget = nil
            originalAction = nil
        }

        func updateMenu() {
            for item in imageMenu.items {
                guard let rawValue = item.representedObject as? String,
                      let filter = ImageTypeFilter(rawValue: rawValue) else { continue }
                item.state = parent.imageType == filter ? .on : .off
                item.isEnabled = filter != .unsure || parent.unsureImageCount > 0
                if filter == .unsure {
                    let count = max(0, parent.unsureImageCount)
                    item.badge = NSMenuItemBadge(count: count)
                    item.setAccessibilityLabel(
                        "Unsure, \(count) \(count == 1 ? "image" : "images") to classify"
                    )
                }
            }
        }

        @objc func selectionChanged(_ sender: NSSegmentedControl) {
            let opensImages = sender.selectedSegment == imagesIndex
            // Keep SwiftUI's original selection handler and transaction intact.
            if let originalAction {
                NSApp.sendAction(originalAction, to: originalTarget, from: sender)
            }
            guard opensImages, !isMenuOpen, sender.window != nil else { return }
            updateMenu()
            let anchor = NSPoint(
                x: sender.bounds.minX + sender.bounds.width / CGFloat(sender.segmentCount),
                y: sender.isFlipped ? sender.bounds.maxY : sender.bounds.minY
            )
            imageMenu.popUp(positioning: nil, at: anchor, in: sender)
        }

        @objc func selectImageType(_ sender: NSMenuItem) {
            guard sender.isEnabled,
                  let rawValue = sender.representedObject as? String,
                  let filter = ImageTypeFilter(rawValue: rawValue) else { return }
            parent.scope = .images
            parent.imageType = filter
            updateMenu()
        }

        func menuWillOpen(_ menu: NSMenu) { isMenuOpen = true }
        func menuDidClose(_ menu: NSMenu) { isMenuOpen = false }
    }
}
