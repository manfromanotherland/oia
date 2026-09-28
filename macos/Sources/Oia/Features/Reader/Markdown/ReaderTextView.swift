// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

extension NSAttributedString.Key {
    /// Marks a range as a saved highlight. `ReaderLayoutManager` fills a tinted
    /// rounded rect behind it. A dedicated key (rather than `.backgroundColor`)
    /// keeps highlights from clobbering inline-code backgrounds.
    static let readerHighlight = NSAttributedString.Key("ReaderHighlight")
}

/// An `NSTextView` that owns its TextKit-1 stack. Building the stack manually
/// (so our custom layout manager is used) means nothing else retains the
/// `NSTextStorage` — the cluster's back-references to it are weak — so the view
/// holds it strongly here to keep the whole stack alive.
final class ReaderTextView: NSTextView {
    private var retainedStorage: NSTextStorage?

    /// The last attributed string set as content, *without* the highlight
    /// attributes layered on top — the baseline `updateNSView` diffs against.
    var baseAttributed: NSAttributedString?

    /// Invoked with the selected text when the user chooses "Highlight".
    var onHighlight: ((String) -> Void)?

    /// Verbatim text of the reading's current highlights, so the context menu
    /// can offer "Remove Highlight" when the selection matches one.
    var highlights: [String] = []

    static func make() -> ReaderTextView {
        let storage = NSTextStorage()
        let layoutManager = ReaderLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = true
        container.lineFragmentPadding = 0
        layoutManager.addTextContainer(container)

        let textView = ReaderTextView(frame: .zero, textContainer: container)
        textView.retainedStorage = storage
        return textView
    }

    /// Height is driven by the laid-out text; width is left flexible so SwiftUI
    /// stretches the view to the proposed width. Because the text container
    /// tracks the view width, `usedRect` reflects wrapping at the current width.
    override var intrinsicContentSize: NSSize {
        guard let layoutManager, let textContainer else { return super.intrinsicContentSize }
        layoutManager.ensureLayout(for: textContainer)
        return NSSize(width: NSView.noIntrinsicMetric,
                      height: ceil(layoutManager.usedRect(for: textContainer).height))
    }

    /// A width change rewraps the text, changing its height — re-measure.
    override func setFrameSize(_ newSize: NSSize) {
        let widthChanged = newSize.width != frame.width
        super.setFrameSize(newSize)
        if widthChanged {
            invalidateIntrinsicContentSize()
        }
    }

    /// Starting an interaction here makes this run the active selection, so clear
    /// any selection held by the other runs first. Each `NSTextView` keeps its own
    /// selection independently, so without this a previous block's selection stays
    /// drawn (grayed, inactive) alongside the new one.
    override func mouseDown(with event: NSEvent) {
        if let content = window?.contentView {
            ReaderTextView.collapseSelections(in: content, except: self)
        }
        super.mouseDown(with: event)
    }

    /// All selection changes funnel through here (including live drag extension
    /// and programmatic clearing). Force a full repaint after each so incremental
    /// selection drawing can't leave stale highlight pixels behind — the ghost
    /// "blue line" that lingered between line fragments during a drag and survived
    /// clearing the selection, because a partial invalidation never covered it.
    override func setSelectedRanges(_ ranges: [NSValue], affinity: NSSelectionAffinity,
                                    stillSelecting stillSelectingFlag: Bool)
    {
        super.setSelectedRanges(ranges, affinity: affinity, stillSelecting: stillSelectingFlag)
        needsDisplay = true
    }

    /// Collapse the selection of every reader text view in `root`'s subtree,
    /// skipping `keep` (the run taking over the selection, if any). Shared with
    /// the margin click-catcher, which clears them all.
    static func collapseSelections(in root: NSView, except keep: ReaderTextView? = nil) {
        for subview in root.subviews {
            if let textView = subview as? ReaderTextView,
               textView !== keep, textView.selectedRange().length > 0
            {
                textView.setSelectedRange(NSRange(location: 0, length: 0))
            }
            collapseSelections(in: subview, except: keep)
        }
    }

    /// The text selected in whichever reader run under `root` holds a selection,
    /// or nil when nothing is selected. Only one run can be selected at a time
    /// (see `collapseSelections`), so the first hit is the answer.
    ///
    /// Lets the toolbar's Highlight button act on the reader from outside its view
    /// hierarchy — the runs are `NSTextView`s SwiftUI can't reach into, so the
    /// selection is found by walking the window, as `focusSearchField()` does.
    static func selectedText(in root: NSView) -> String? {
        for subview in root.subviews {
            if let textView = subview as? ReaderTextView {
                let range = textView.selectedRange()
                if range.length > 0, let storage = textView.textStorage {
                    return (storage.string as NSString).substring(with: range)
                }
            }
            if let found = selectedText(in: subview) {
                return found
            }
        }
        return nil
    }

    /// Keep the "Services" submenu out of the reader's context menu. AppKit adds
    /// it to a text view's menu because the view advertises that it can hand its
    /// selection to a service; reporting no valid send/return types removes the
    /// entry. Copy and the reader's own Highlight / Look Up commands don't go
    /// through the Services architecture, so they are unaffected.
    override func validRequestor(forSendType _: NSPasteboard.PasteboardType?,
                                 returnType _: NSPasteboard.PasteboardType?) -> Any?
    {
        nil
    }

    /// Replace the default rich text-editing context menu with just the commands
    /// the reader offers: a highlight toggle, and a "Look Up" that opens the
    /// system dictionary panel for the selection. With no selection there is
    /// nothing to act on, so no menu is shown. When the selection exactly matches
    /// an existing highlight the highlight command removes it; otherwise it adds
    /// one.
    override func menu(for _: NSEvent) -> NSMenu? {
        let range = selectedRange()
        guard range.length > 0, let storage = textStorage else { return nil }
        let selected = (storage.string as NSString)
            .substring(with: range)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let menu = NSMenu()

        let highlightTitle = highlights.contains(selected) ? "Remove Highlight" : "Highlight"
        let highlightItem = NSMenuItem(title: highlightTitle,
                                       action: #selector(highlightSelection(_:)),
                                       keyEquivalent: "")
        highlightItem.target = self
        menu.addItem(highlightItem)

        // "Look Up" opens the native define/thesaurus/Wikipedia popover. Only
        // offered when the trimmed selection has content to define; grouped in
        // its own section, matching how macOS separates Look Up from edit
        // actions.
        if !selected.isEmpty {
            menu.addItem(.separator())
            let lookUpItem = NSMenuItem(title: "Look Up \(Self.lookUpLabel(for: selected))",
                                        action: #selector(lookUpSelection(_:)),
                                        keyEquivalent: "")
            lookUpItem.target = self
            menu.addItem(lookUpItem)
        }
        return menu
    }

    @objc private func highlightSelection(_: Any?) {
        let range = selectedRange()
        guard range.length > 0, let storage = textStorage else { return }
        let text = (storage.string as NSString).substring(with: range)
        onHighlight?(text)
    }

    /// Show the system Look Up panel (Dictionary, Thesaurus, and Apple's
    /// knowledge sources) for the current selection, anchored at its baseline.
    @objc private func lookUpSelection(_: Any?) {
        let range = selectedRange()
        guard range.length > 0,
              let storage = textStorage,
              let layoutManager,
              let container = textContainer else { return }
        let glyphRange = layoutManager.glyphRange(forCharacterRange: range,
                                                  actualCharacterRange: nil)
        let bounds = layoutManager.boundingRect(forGlyphRange: glyphRange, in: container)
        // The panel wants the baseline origin of the first character; the bottom
        // of the flipped line rect approximates it well enough to anchor there.
        let origin = NSPoint(x: bounds.minX + textContainerOrigin.x,
                             y: bounds.maxY + textContainerOrigin.y)
        showDefinition(for: storage.attributedSubstring(from: range), at: origin)
    }

    /// A quoted, length-capped rendering of the selection for the "Look Up" menu
    /// title, mirroring the system item's `Look Up “word”` styling.
    private static func lookUpLabel(for text: String) -> String {
        let maxLength = 24
        let condensed = text.replacingOccurrences(of: "\n", with: " ")
        guard condensed.count > maxLength else { return "“\(condensed)”" }
        return "“\(condensed.prefix(maxLength))…”"
    }
}

/// Draws saved highlights behind the selectable article text.
final class ReaderLayoutManager: NSLayoutManager {
    /// Translucent fill behind highlighted passages. Yellow reads as a marker
    /// in both light and dark mode at this alpha.
    private let highlightColor = NSColor.systemYellow.withAlphaComponent(0.32)

    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
        guard let textStorage else { return }

        drawHighlights(forGlyphRange: glyphsToShow, at: origin, textStorage: textStorage)
    }

    /// Fill a rounded tint behind every range carrying `.readerHighlight`. Drawn
    /// in `drawBackground` (before the glyphs) so text stays on top.
    private func drawHighlights(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint,
                                textStorage: NSTextStorage)
    {
        guard let container = textContainers.first else { return }
        let charRange = characterRange(forGlyphRange: glyphsToShow, actualGlyphRange: nil)
        highlightColor.setFill()
        textStorage.enumerateAttribute(.readerHighlight, in: charRange) { value, range, _ in
            guard value != nil else { return }
            let glyphRange = self.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
            self.enumerateEnclosingRects(
                forGlyphRange: glyphRange,
                withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0),
                in: container
            ) { rect, _ in
                let tintRect = NSRect(x: origin.x + rect.minX, y: origin.y + rect.minY,
                                      width: rect.width, height: rect.height)
                NSBezierPath(roundedRect: tintRect, xRadius: 2, yRadius: 2).fill()
            }
        }
    }
}
