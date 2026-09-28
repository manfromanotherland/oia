// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// A read-only `NSTextView` bridged into SwiftUI. SwiftUI's `Text` +
/// `.textSelection(.enabled)` can only select *within* a single `Text`, so the
/// reader (which renders one `Text` per block) cannot drag-select across
/// paragraphs. Rendering a contiguous run of text blocks as one
/// `NSAttributedString` in an `NSTextView` restores native, continuous
/// selection (and copy) across the whole run.
///
/// The view draws no background and no insets so it lines up with the
/// surrounding SwiftUI blocks; intrinsic height follows the laid-out text.
/// A custom `ReaderLayoutManager` draws saved highlights.
struct SelectableTextView: NSViewRepresentable {
    let attributed: NSAttributedString
    /// Verbatim text of every highlight for this reading. Each exact occurrence
    /// found in this run is tinted.
    var highlights: [String] = []
    /// Called with the selected text when the user picks "Highlight" from the
    /// context menu.
    var onHighlight: (String) -> Void = { _ in }

    func makeNSView(context _: Context) -> ReaderTextView {
        let textView = ReaderTextView.make()
        textView.onHighlight = onHighlight
        textView.highlights = highlights
        textView.isEditable = false
        textView.isSelectable = true
        textView.drawsBackground = false
        textView.backgroundColor = .clear
        textView.textContainerInset = .zero
        textView.isHorizontallyResizable = false
        textView.isVerticallyResizable = true
        textView.minSize = .zero
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.autoresizingMask = []
        // Links are opened by NSTextView's default handling (NSWorkspace), which
        // matches the rest of the reader: links open in the system browser.
        // The view's link attributes override the source color during layout.
        textView.linkTextAttributes = [
            .foregroundColor: NSColor.labelColor,
            .underlineStyle: NSUnderlineStyle.single.rawValue,
            .cursor: NSCursor.pointingHand
        ]
        // Fill the width SwiftUI proposes; height comes from intrinsicContentSize.
        textView.setContentHuggingPriority(.defaultLow, for: .horizontal)
        textView.setContentHuggingPriority(.defaultHigh, for: .vertical)
        textView.setContentCompressionResistancePriority(.required, for: .vertical)
        return textView
    }

    func updateNSView(_ textView: ReaderTextView, context _: Context) {
        // Only re-set the storage when the *base* content actually changed (e.g.
        // the user changed the reader font/size). Re-setting on every SwiftUI
        // pass would clear the user's in-progress selection. We compare against
        // the stored base (not the live storage, which carries highlight
        // attributes added below) so applying a tint never looks like a content
        // change. Invalidating the intrinsic size makes SwiftUI re-read the
        // height *after* the text is in place — without this, a run measured
        // before its text was set (which happens on the first layout pass)
        // collapses to zero height and renders blank.
        guard let storage = textView.textStorage else { return }
        textView.onHighlight = onHighlight
        textView.highlights = highlights
        if textView.baseAttributed?.isEqual(attributed) != true {
            storage.setAttributedString(attributed)
            textView.baseAttributed = attributed.copy() as? NSAttributedString
            textView.invalidateIntrinsicContentSize()
        }
        applyHighlightTints(to: storage, in: textView)
    }

    /// Tag every exact occurrence of each highlight string with
    /// `.readerHighlight` so the layout manager draws a tint behind it. Cheap
    /// for article-sized text; runs each pass so added/removed highlights show
    /// immediately without rebuilding the attributed string.
    private func applyHighlightTints(to storage: NSTextStorage, in textView: ReaderTextView) {
        let whole = NSRange(location: 0, length: storage.length)
        storage.removeAttribute(.readerHighlight, range: whole)
        let haystack = storage.string as NSString
        for needle in highlights where !needle.isEmpty {
            var searchStart = 0
            while searchStart < haystack.length {
                let scope = NSRange(location: searchStart, length: haystack.length - searchStart)
                let found = haystack.range(of: needle, options: [], range: scope)
                if found.location == NSNotFound {
                    break
                }
                storage.addAttribute(.readerHighlight, value: true, range: found)
                searchStart = found.location + max(found.length, 1)
            }
        }
        textView.needsDisplay = true
    }
}

/// A transparent click-catcher placed *behind* the article content. A reader
/// text run keeps its selection until something clears it, but clicking the
/// margins around the text — or the gaps between blocks — lands on the scroll
/// view's empty background, which no text view sees, so the selection lingers.
/// This view fills that background and, on a click that misses the text views on
/// top, collapses any active selection — so clicking outside the text deselects,
/// as it would in a single document. It sits behind the runs, so it only ever
/// receives clicks the text views did not handle and never interferes with
/// selecting, clicking links, or the context menu.
struct SelectionClearingBackground: NSViewRepresentable {
    func makeNSView(context _: Context) -> NSView {
        ClickCatcher()
    }

    func updateNSView(_: NSView, context _: Context) {}

    private final class ClickCatcher: NSView {
        /// Deselect even when the window is not key, so a single click both
        /// activates the window and clears the selection.
        override func acceptsFirstMouse(for _: NSEvent?) -> Bool {
            true
        }

        override func mouseDown(with _: NSEvent) {
            guard let content = window?.contentView else { return }
            ReaderTextView.collapseSelections(in: content)
        }
    }
}
