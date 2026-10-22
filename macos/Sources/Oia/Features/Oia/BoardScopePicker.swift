// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Keeps the board's native segmented control and gives Images a native menu.
struct BoardScopePicker: NSViewRepresentable {
    @Binding var scope: LibraryScope
    @Binding var imageType: ImageTypeFilter
    let unsureImageCount: Int

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSSegmentedControl {
        context.coordinator.makeControl()
    }

    func updateNSView(_ control: NSSegmentedControl, context: Context) {
        context.coordinator.parent = self
        context.coordinator.update(control)
    }

    @MainActor
    final class Coordinator: NSObject, NSMenuDelegate {
        var parent: BoardScopePicker
        let imageMenu = NSMenu(title: "Images")
        private weak var control: NSSegmentedControl?
        private var isMenuOpen = false
        private let scopes = LibraryScope.allCases

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
        }

        func makeControl() -> NSSegmentedControl {
            let control = NSSegmentedControl(
                labels: scopes.map(\.label), trackingMode: .selectOne,
                target: self, action: #selector(selectScope(_:))
            )
            control.segmentStyle = .automatic
            control.segmentDistribution = .fillEqually
            control.setMenu(imageMenu, forSegment: imagesIndex)
            control.setShowsMenuIndicator(true, forSegment: imagesIndex)
            control.setAccessibilityLabel("Filter")
            control.setAccessibilityIdentifier(A11y.Filter.group)
            self.control = control
            update(control)
            return control
        }

        func update(_ control: NSSegmentedControl) {
            control.selectedSegment = scopes.firstIndex(of: parent.scope) ?? 0
            for (index, scope) in scopes.enumerated() {
                let tip = scope == .images
                    ? "Images: \(parent.imageType == .all ? "All Images" : parent.imageType.label)"
                    : "Show \(scope.label.lowercased())"
                control.setToolTip(tip, forSegment: index)
            }
            updateMenu()
        }

        private var imagesIndex: Int { scopes.firstIndex(of: .images)! }

        private func updateMenu() {
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

        @objc func selectScope(_ sender: NSSegmentedControl) {
            guard scopes.indices.contains(sender.selectedSegment) else { return }
            let scope = scopes[sender.selectedSegment]
            parent.scope = scope
            guard scope == .images, !isMenuOpen, sender.window != nil else { return }

            // AppKit provides press-and-hold for a segment with both an action
            // and a menu. Open the same menu on an ordinary click as well.
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
            if let control { update(control) }
        }

        func menuWillOpen(_ menu: NSMenu) {
            isMenuOpen = true
            parent.scope = .images
            updateMenu()
        }

        func menuDidClose(_ menu: NSMenu) {
            isMenuOpen = false
            // A menu segment is momentary while AppKit tracks it. Reconcile its
            // highlight after tracking restores the previous native selection.
            DispatchQueue.main.async { [weak self] in
                guard let self, let control = self.control else { return }
                self.update(control)
            }
        }
    }
}
