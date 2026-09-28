// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Supplies a compact resting width to SwiftUI's native
/// `NSSearchToolbarItem`. AppKit still owns the field, focus, cancel behavior,
/// and expanded width.
struct CompactSearchToolbarConfiguration: NSViewRepresentable {
    let isSearchExpanded: Bool
    let searchTokens: [BoardSearchToken]

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
        SearchToolbarConfigurationView(
            isSearchExpanded: isSearchExpanded,
            searchTokens: searchTokens
        )
    }

    func updateNSView(_ view: NSView, context _: Context) {
        (view as? SearchToolbarConfigurationView)?.update(
            isSearchExpanded: isSearchExpanded,
            searchTokens: searchTokens
        )
    }

    @MainActor
    private final class SearchToolbarConfigurationView: NSView {
        private static let compactConstraintIdentifier =
            "is.edmundo.oia.search.compact-resting-width"
        private static let trailingIdentifier =
            "is.edmundo.oia.search.trailing-toolbar-edge"
        private var isSearchExpanded: Bool
        private var searchTokens: [BoardSearchToken]
        nonisolated(unsafe) private var mouseMonitor: Any?

        init(
            isSearchExpanded: Bool,
            searchTokens: [BoardSearchToken]
        ) {
            self.isSearchExpanded = isSearchExpanded
            self.searchTokens = searchTokens
            super.init(frame: .zero)

            NotificationCenter.default.addObserver(
                self,
                selector: #selector(toolbarWillAddItem(_:)),
                name: NSToolbar.willAddItemNotification,
                object: nil
            )
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(searchTextDidChange(_:)),
                name: NSControl.textDidChangeNotification,
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

        func update(
            isSearchExpanded: Bool,
            searchTokens: [BoardSearchToken]
        ) {
            self.isSearchExpanded = isSearchExpanded
            self.searchTokens = searchTokens
            configureCurrentToolbar()
            DispatchQueue.main.async { [weak self] in
                self?.decorateColorTokens()
            }
        }

        private func decorateColorTokens() {
            guard let field = window?.toolbar?.items
                .compactMap({ $0 as? NSSearchToolbarItem }).first?.searchField
            else { return }
            func decorate(_ attributed: NSAttributedString) {
                var index = 0
                attributed.enumerateAttribute(
                    .attachment,
                    in: NSRange(location: 0, length: attributed.length)
                ) { value, _, _ in
                    guard let attachment = value as? NSTextAttachment else { return }
                    defer { index += 1 }
                    guard let cell = attachment.attachmentCell else { return }
                    let nativeCell = (cell as? ColorSearchTokenCell)?.original ?? cell
                    guard searchTokens.indices.contains(index),
                          searchTokens[index].kind == .color,
                          let palette = CardThemePalette(themeColor: searchTokens[index].value)
                    else {
                        if cell is ColorSearchTokenCell { attachment.attachmentCell = nativeCell }
                        return
                    }
                    if let colored = cell as? ColorSearchTokenCell,
                       colored.matches(title: searchTokens[index].displayValue, palette: palette)
                    { return }
                    attachment.attachmentCell = ColorSearchTokenCell(
                        original: nativeCell,
                        title: searchTokens[index].displayValue,
                        palette: palette
                    )
                }
            }
            decorate(field.attributedStringValue)
            let editor = field.currentEditor() as? NSTextView
            if let content = editor?.textStorage { decorate(content) }
            field.needsDisplay = true
            editor?.needsDisplay = true
        }

        private func prepareCollapse(_ item: NSSearchToolbarItem) {
            isSearchExpanded = false
            configureCurrentToolbar()
            item.endSearchInteraction()
        }

        func configureCurrentToolbar() {
            window?.titlebarAppearsTransparent = true
            window?.titlebarSeparatorStyle = .none
            window?.toolbar?.items
                .compactMap { $0 as? NSSearchToolbarItem }
                .forEach { configure($0) }
            decorateColorTokens()
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

        @objc private func searchTextDidChange(_ notification: Notification) {
            guard let field = window?.toolbar?.items
                .compactMap({ $0 as? NSSearchToolbarItem }).first?.searchField,
                notification.object as AnyObject? === field
            else { return }
            DispatchQueue.main.async { [weak self] in
                self?.decorateColorTokens()
            }
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

/// The native search field renders SwiftUI tokens as text attachments. Its
/// attachment cell owns the visible chip, so SwiftUI view backgrounds on
/// the token label do not reach the search field.
@MainActor
private final class ColorSearchTokenCell: NSTextAttachmentCell {
    nonisolated(unsafe) let original: any NSTextAttachmentCellProtocol
    private let tokenTitle: String
    private let palette: CardThemePalette

    init(original: any NSTextAttachmentCellProtocol, title: String, palette: CardThemePalette) {
        self.original = original
        tokenTitle = title
        self.palette = palette
        super.init(textCell: "")
    }

    @available(*, unavailable)
    required init(coder _: NSCoder) {
        fatalError("ColorSearchTokenCell cannot be decoded")
    }

    override var cellSize: NSSize { original.cellSize() }

    func matches(title: String, palette: CardThemePalette) -> Bool {
        tokenTitle == title && self.palette == palette
    }

    override func cellBaselineOffset() -> NSPoint { original.cellBaselineOffset() }
    override func wantsToTrackMouse() -> Bool { original.wantsToTrackMouse() }

    override func draw(withFrame cellFrame: NSRect, in controlView: NSView?) {
        original.draw(withFrame: cellFrame, in: controlView)
        let background = palette.background
        NSColor(srgbRed: background.red, green: background.green, blue: background.blue, alpha: 1)
            .setFill()
        let chipFrame = NSRect(
            x: cellFrame.minX + 2,
            y: cellFrame.minY + 1,
            width: cellFrame.width - 4,
            height: cellFrame.height - 1
        )
        NSBezierPath(roundedRect: chipFrame, xRadius: 2.5, yRadius: 2.5).fill()

        let foreground = palette.foreground
        let textColor = NSColor(
            srgbRed: foreground.red,
            green: foreground.green,
            blue: foreground.blue,
            alpha: 0.8
        )
        let font = (original as? NSCell)?.font ?? NSFont.systemFont(ofSize: NSFont.systemFontSize)
        let text = NSAttributedString(string: tokenTitle, attributes: [
            .font: font,
            .foregroundColor: textColor
        ])
        let textSize = text.size()
        text.draw(at: NSPoint(
            x: cellFrame.midX - textSize.width / 2,
            y: cellFrame.midY - textSize.height / 2
        ))
    }

    override func highlight(_ flag: Bool, withFrame cellFrame: NSRect, in controlView: NSView?) {
        original.highlight(flag, withFrame: cellFrame, in: controlView)
    }

    override func trackMouse(
        with event: NSEvent,
        in cellFrame: NSRect,
        of controlView: NSView?,
        untilMouseUp flag: Bool
    ) -> Bool {
        original.trackMouse(with: event, in: cellFrame, of: controlView, untilMouseUp: flag)
    }
}
