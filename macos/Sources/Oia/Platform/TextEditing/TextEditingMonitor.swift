// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import WebKit

/// Tracks whether the user is editing a text field (the toolbar search field,
/// the Inspector tag input, …). macOS dispatches menu/context-menu key-equivalents
/// *before* the focused field editor, so a global shortcut can fire instead of
/// editing the line; commands whose shortcuts collide with the field editor's
/// own keys disable themselves while a field is focused. Rather than wire focus
/// into every field, this watches the field
/// editor's begin/end-editing notifications (and key window changes) and
/// re-reads the key window's first responder — one place, covering the search
/// field, the Inspector tag input, and any field added later.
@MainActor
final class TextEditingMonitor {
    /// Publishes native editing and web content focus independently.
    private let onChange: (Bool, Bool) -> Void

    private var context = KeyboardContext()

    /// Tokens for the focus observers; removed in `deinit`.
    /// `nonisolated(unsafe)` so the `nonisolated deinit` can read them to tear
    /// down: they're only written on the main actor (in `init`) and read once at
    /// deinit, which has exclusive access — so there's no actual race to guard.
    private nonisolated(unsafe) var observers: [NSObjectProtocol] = []

    init(onChange: @escaping (Bool, Bool) -> Void) {
        self.onChange = onChange
        let names: [Notification.Name] = [
            NSText.didBeginEditingNotification,
            NSText.didEndEditingNotification,
            NSWindow.didBecomeKeyNotification,
            NSWindow.didResignKeyNotification,
            // WebKit's content responder is not an NSText and does not publish
            // field-editor notifications. Window updates run after focus changes
            // from clicks, Tab, and page scripts; only changed contexts publish.
            NSWindow.didUpdateNotification
        ]
        let center = NotificationCenter.default
        observers = names.map { name in
            center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                // Delivery on `.main` runs on the main thread, so it's safe to
                // assume MainActor isolation and read AppKit/UI state directly.
                MainActor.assumeIsolated { self?.refresh() }
            }
        }
    }

    deinit {
        let center = NotificationCenter.default
        observers.forEach { center.removeObserver($0) }
    }

    private func refresh() {
        let next = Self.keyboardContext(for: NSApp.keyWindow?.firstResponder)
        if next != context {
            context = next
            onChange(next.isEditingText, next.isBrowsingWebContent)
        }
    }

    struct KeyboardContext: Equatable {
        var isEditingText = false
        var isBrowsingWebContent = false
    }

    /// Whether the first responder is an editable text input — the
    /// field editor behind a `TextField`/search field, or an editable `NSTextView`.
    /// The reader's selectable-but-read-only text view is intentionally excluded:
    /// there's no line to delete there, so its shortcuts should keep working.
    /// WebKit owns its own text input and page keyboard behavior. Track it
    /// separately so toolbar buttons remain available while conflicting app
    /// key equivalents defer to the page.
    static func keyboardContext(for responder: NSResponder?) -> KeyboardContext {
        guard let responder else { return KeyboardContext() }
        var view = responder as? NSView
        while let current = view {
            if current is WKWebView {
                return KeyboardContext(isBrowsingWebContent: true)
            }
            view = current.superview
        }
        if let textView = responder as? NSTextView {
            return KeyboardContext(isEditingText: textView.isFieldEditor || textView.isEditable)
        }
        return KeyboardContext(isEditingText: responder is NSText)
    }
}
