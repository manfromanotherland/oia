// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

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
    /// Called with the new value whenever "a text field is focused" flips.
    private let onChange: (Bool) -> Void

    private var isEditing = false

    /// Tokens for the focus observers; removed in `deinit`.
    /// `nonisolated(unsafe)` so the `nonisolated deinit` can read them to tear
    /// down: they're only written on the main actor (in `init`) and read once at
    /// deinit, which has exclusive access — so there's no actual race to guard.
    private nonisolated(unsafe) var observers: [NSObjectProtocol] = []

    init(onChange: @escaping (Bool) -> Void) {
        self.onChange = onChange
        let names: [Notification.Name] = [
            NSText.didBeginEditingNotification,
            NSText.didEndEditingNotification,
            NSWindow.didBecomeKeyNotification,
            NSWindow.didResignKeyNotification
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
        let editing = Self.firstResponderIsTextInput()
        if editing != isEditing {
            isEditing = editing
            onChange(editing)
        }
    }

    /// Whether the key window's first responder is an editable text input — the
    /// field editor behind a `TextField`/search field, or an editable `NSTextView`.
    /// The reader's selectable-but-read-only text view is intentionally excluded:
    /// there's no line to delete there, so its shortcuts should keep working.
    private static func firstResponderIsTextInput() -> Bool {
        guard let responder = NSApp.keyWindow?.firstResponder else { return false }
        if let textView = responder as? NSTextView {
            return textView.isFieldEditor || textView.isEditable
        }
        return responder is NSText
    }
}
