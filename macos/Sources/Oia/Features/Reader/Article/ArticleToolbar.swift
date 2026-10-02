// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// The reader's primary-action toolbar: open in browser, highlight, and
/// delete. The parent resolves which row to build
/// it for (see `ArticleDetailView.currentRow`); actions write through
/// `appState`, whose refresh supplies the updated row on the next render.
struct ArticleToolbar: ToolbarContent {
    let row: ReadingRow
    let appState: AppState

    var body: some ToolbarContent {
        // Local binding for the hint popover; `appState` is a plain stored
        // reference here, not an `@Environment` value.
        @Bindable var appState = appState
        ToolbarItemGroup(placement: .primaryAction) {
            // A `Spacer` in a macOS toolbar is an expanding flexible space; it
            // pushes these actions to the trailing edge (far right).
            Spacer()

            if let url = row.sourceURL {
                Button {
                    ReadingLink.open(url)
                } label: {
                    Label("Open in Browser", systemImage: "safari")
                }
                .help("Open original URL")
                .accessibilityIdentifier(A11y.Toolbar.openInBrowser)
            }

            Button {
                // Toggles, like the reader's context-menu command: pressing it
                // over an already-highlighted passage clears it. The inspector is
                // reached from the Article menu (⌘⇧H) instead.
                if !appState.highlightSelection() {
                    appState.showHighlightHint = true
                }
            } label: {
                Label("Highlight", systemImage: "highlighter")
            }
            .help("Highlight the selected text")
            .accessibilityIdentifier(A11y.Toolbar.highlight)
            .popover(isPresented: $appState.showHighlightHint, arrowEdge: .bottom) {
                Text("Select some text in the article to highlight it.")
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .accessibilityIdentifier(A11y.Toolbar.highlightHint)
            }

            Button(role: .destructive) {
                appState.requestDelete(row)
            } label: {
                Label("Delete", systemImage: "trash")
            }
            .help("Permanently delete this item")
            .accessibilityIdentifier(A11y.Toolbar.delete)
            .disabled(appState.isDeleting)

            Button {
                appState.isFocusMode.toggle()
            } label: {
                Label(
                    appState.isFocusMode ? "Exit Focus" : "Focus",
                    systemImage: appState.isFocusMode
                        ? "arrow.down.right.and.arrow.up.left"
                        : "arrow.up.left.and.arrow.down.right"
                )
            }
            .keyboardShortcut(ShortcutCatalog.toggleFocusMode)
            .help(appState.isFocusMode ? "Exit focus mode" : "Enter focus mode")
        }
    }
}
