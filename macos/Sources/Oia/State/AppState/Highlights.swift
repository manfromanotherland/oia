// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

// ── Highlights ───────────────────────────────────────────────────────────────

extension AppState {
    /// Toggle a highlight over the reader's current text selection — the toolbar
    /// button's action, matching the reader context menu's Highlight command.
    ///
    /// Returns false when there's nothing to act on, so the caller can prompt the
    /// user to select something. The selection lives in AppKit text views outside
    /// SwiftUI's reach; `ReaderTextView.selectedText(in:)` finds it.
    @discardableResult
    func highlightSelection() -> Bool {
        guard let id = selectedId,
              let root = (NSApp.keyWindow ?? NSApp.mainWindow)?.contentView,
              let text = ReaderTextView.selectedText(in: root)
        else { return false }
        Task { await toggleHighlight(id: id, text: text) }
        return true
    }

    /// Load the highlights for `id` into `highlights`. Pass `nil` to clear
    /// (e.g. when no reading is selected).
    func loadHighlights(id: String?) async {
        guard let core, let id else { highlights = []; return }
        let loaded = await (try? core.listHighlights(readingId: id)) ?? []
        // The reader no longer awaits this (see `loadHighlightsInBackground`), so a
        // slow fetch can land after the user has moved to another reading — don't let
        // it overwrite that one's highlights.
        guard selectedId == id else { return }
        highlights = loaded.map { HighlightRow($0) }
    }

    /// Save a new highlight for `id` from the user's selected text, then reload.
    func addHighlight(id: String, text: String) async {
        guard let core else { return }
        beginLibraryWrite()
        defer { endLibraryWrite() }
        do {
            try await core.addHighlight(readingId: id, text: text)
            await loadHighlights(id: id)
        } catch {
            self.error = error.localizedDescription
        }
        await refresh()
    }

    /// Toggle a highlight for `id`: clears it if the exact passage is already
    /// highlighted, otherwise adds it. Reloads either way.
    func toggleHighlight(id: String, text: String) async {
        guard let core else { return }
        if let highlight = highlights.first(where: { $0.text == text.trimmingCharacters(in: .whitespacesAndNewlines) }) {
            removeHighlightOptimistically(readingId: id, highlightId: highlight.id)
        }
        beginLibraryWrite()
        defer { endLibraryWrite() }
        do {
            try await core.toggleHighlight(readingId: id, text: text)
            await loadHighlights(id: id)
        } catch {
            self.error = error.localizedDescription
        }
        await refresh()
    }

    /// Remove a single highlight from `id`, then reload.
    func deleteHighlight(id: String, highlightId: String) async {
        guard let core else { return }
        removeHighlightOptimistically(readingId: id, highlightId: highlightId)
        beginLibraryWrite()
        defer { endLibraryWrite() }
        try? await core.deleteHighlight(readingId: id, highlightId: highlightId)
        await loadHighlights(id: id)
        await refresh()
    }

    private func removeHighlightOptimistically(readingId: String, highlightId: String) {
        guard selectedId == readingId,
              let removed = highlights.first(where: { $0.id == highlightId })
        else { return }
        highlights.removeAll { $0.id == highlightId }
        if let index = readings.firstIndex(where: { $0.id == readingId }),
           readings[index].featuredHighlightText == removed.text
        {
            readings[index].featuredHighlightText = highlights.last?.text
        }
    }
}
