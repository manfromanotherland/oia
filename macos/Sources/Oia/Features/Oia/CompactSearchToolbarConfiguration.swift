// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Supplies a compact resting width to SwiftUI's native
/// `NSSearchToolbarItem`. AppKit still owns the field, focus, cancel behavior,
/// and expanded width.
struct CompactSearchToolbarConfiguration: NSViewRepresentable {
    let isSearchExpanded: Bool

    /// Give the toolbar its expanded allocation before moving focus to the field.
    /// Focusing first lets AppKit draw the full field past the window's right edge
    /// while the toolbar is still laid out at the compact width.
    static func beginSearchInteraction(in window: NSWindow?) {
        guard let item = window?.toolbar?.items
            .compactMap({ $0 as? NSSearchToolbarItem }).first else { return }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0
            context.allowsImplicitAnimation = false
            SearchToolbarConfigurationView.expand(item, in: window)
            item.beginSearchInteraction()
            window?.displayIfNeeded()
        }
    }

    func makeNSView(context _: Context) -> NSView {
        SearchToolbarConfigurationView(isSearchExpanded: isSearchExpanded)
    }

    func updateNSView(_ view: NSView, context _: Context) {
        (view as? SearchToolbarConfigurationView)?.setSearchExpanded(isSearchExpanded)
    }

    @MainActor
    private final class SearchToolbarConfigurationView: NSView {
        private static let compactConstraintIdentifier =
            "is.edmundo.oia.search.compact-resting-width"
        private static let trailingIdentifier =
            "is.edmundo.oia.search.trailing-toolbar-edge"
        private var isSearchExpanded: Bool
        nonisolated(unsafe) private var mouseMonitor: Any?

        init(isSearchExpanded: Bool) {
            self.isSearchExpanded = isSearchExpanded
            super.init(frame: .zero)

            NotificationCenter.default.addObserver(
                self,
                selector: #selector(toolbarWillAddItem(_:)),
                name: NSToolbar.willAddItemNotification,
                object: nil
            )
        }

        @available(*, unavailable)
        required init?(coder _: NSCoder) {
            nil
        }

        deinit {
            NotificationCenter.default.removeObserver(self)
            if let mouseMonitor { NSEvent.removeMonitor(mouseMonitor) }
        }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let mouseMonitor { NSEvent.removeMonitor(mouseMonitor) }
            mouseMonitor = nil
            if window != nil {
                mouseMonitor = NSEvent.addLocalMonitorForEvents(matching: .leftMouseDown) {
                    [weak self] event in
                    guard let self, event.window === self.window,
                          let item = self.window?.toolbar?.items
                            .compactMap({ $0 as? NSSearchToolbarItem }).first
                    else { return event }

                    let field = item.searchField
                    let clickedSearch = field.bounds.contains(
                        field.convert(event.locationInWindow, from: nil)
                    )
                    if !self.isSearchExpanded, clickedSearch {
                        self.isSearchExpanded = true
                        CompactSearchToolbarConfiguration.beginSearchInteraction(in: self.window)
                        return nil
                    }

                    if self.isSearchExpanded, !clickedSearch,
                       field.stringValue.isEmpty
                    {
                        self.prepareCollapse(item)
                    }

                    return event
                }
            }
            configureCurrentToolbar()

            // SwiftUI can install its default search item after attaching the
            // content view. Recheck on the next main-loop turn as well as via
            // the toolbar-item notification above.
            DispatchQueue.main.async { [weak self] in
                self?.configureCurrentToolbar()
            }
        }

        override func hitTest(_: NSPoint) -> NSView? {
            nil
        }

        func setSearchExpanded(_ isSearchExpanded: Bool) {
            self.isSearchExpanded = isSearchExpanded
            configureCurrentToolbar()
        }

        private func prepareCollapse(_ item: NSSearchToolbarItem) {
            isSearchExpanded = false
            configureCurrentToolbar()
            item.endSearchInteraction()
        }

        func configureCurrentToolbar() {
            window?.toolbar?.items
                .compactMap { $0 as? NSSearchToolbarItem }
                .forEach { configure($0) }
        }

        @objc private func toolbarWillAddItem(_ notification: Notification) {
            guard let toolbar = notification.object as? NSToolbar,
                  toolbar === window?.toolbar,
                  let item = notification.userInfo?[NSToolbarUserInfoKey.itemKey]
                  as? NSSearchToolbarItem
            else {
                return
            }

            configure(item)
        }

        private func configure(_ item: NSSearchToolbarItem) {
            let field = item.searchField
            if let searchItemView = field.superview,
               let toolbarItemViewer = searchItemView.superview
            {
                // AppKit centers the search view in the toolbar viewer. The
                // viewer becomes compact before the field's native shrink
                // animation finishes, which centers the wide field beyond
                // the window edge. Let the trailing constraint win while the
                // native field animates its width.
                toolbarItemViewer.constraints.first(where: {
                    $0.firstItem === searchItemView &&
                    $0.secondItem === toolbarItemViewer &&
                    $0.firstAttribute == .centerX
                })?.priority = .defaultHigh
                if !toolbarItemViewer.constraints.contains(where: {
                    $0.identifier == Self.trailingIdentifier
                }) {
                    let trailing = searchItemView.trailingAnchor.constraint(
                        equalTo: toolbarItemViewer.trailingAnchor, constant: -4
                    )
                    trailing.identifier = Self.trailingIdentifier
                    trailing.priority = .init(rawValue: 999)
                    trailing.isActive = true
                }
            }
            if let compactWidth = field.constraints.first(where: {
                $0.identifier == Self.compactConstraintIdentifier
            }) {
                updatePriority(of: compactWidth, in: field)
                return
            }

            let compactWidth = field.widthAnchor.constraint(equalTo: field.heightAnchor)
            compactWidth.identifier = Self.compactConstraintIdentifier
            // AppKit's resting autoresizing width uses `.defaultHigh`. Prefer
            // the compressed native representation by one point only while
            // search is collapsed. While focused or retaining search input,
            // dropping this below AppKit's width lets the trailing item expand
            // leftward without clipping.
            compactWidth.priority = desiredPriority
            compactWidth.isActive = true
        }

        private var desiredPriority: NSLayoutConstraint.Priority {
            isSearchExpanded
                ? .defaultLow
                : .init(rawValue: NSLayoutConstraint.Priority.defaultHigh.rawValue + 1)
        }

        private func updatePriority(
            of compactWidth: NSLayoutConstraint,
            in field: NSSearchField
        ) {
            let priority = desiredPriority
            guard compactWidth.priority != priority else { return }

            if !isSearchExpanded {
                compactWidth.priority = priority
                return
            }

            // Expand before focus enters the field so the incoming native
            // animation begins with the toolbar's full allocation.
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0
                context.allowsImplicitAnimation = false
                compactWidth.constant = 0
                compactWidth.priority = priority
                // The toolbar lives above the content view. Resolve its new
                // allocation before AppKit animates the field toward the
                // compact width, or the trailing edge briefly leaves the window.
                window?.contentView?.superview?.layoutSubtreeIfNeeded()
                window?.displayIfNeeded()
            }
        }

        static func expand(_ item: NSSearchToolbarItem, in window: NSWindow?) {
            let field = item.searchField
            guard let compactWidth = field.constraints.first(where: {
                $0.identifier == compactConstraintIdentifier
            }) else { return }
            compactWidth.constant = 0
            compactWidth.priority = .defaultLow
            window?.contentView?.superview?.layoutSubtreeIfNeeded()
            window?.displayIfNeeded()
        }
    }
}
