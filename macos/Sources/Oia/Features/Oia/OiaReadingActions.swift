// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Actions in a card's context menu. Mutations still flow through `AppState`.
struct OiaReadingActions: View {
    @Environment(AppState.self) private var appState

    let row: ReadingRow
    var onEditTags: () -> Void

    var body: some View {
        Button("Copy") {
            ReadingClipboard.copyFile(row, libraryURL: appState.libraryURL)
        }
        .keyboardShortcut(ShortcutCatalog.copy)
        .disabled(ReadingClipboard.fileURL(for: row, libraryURL: appState.libraryURL) == nil)

        Divider()

        Button("Edit Tags…") { onEditTags() }
            .keyboardShortcut(ShortcutCatalog.editTags)
            .disabled(disablesSingleReadingActions || appState.isDeleting)

        if let url = row.sourceURL {
            Button("Open Source") {
                ReadingLink.open(url)
            }
            .keyboardShortcut(ShortcutCatalog.openInBrowser)
            .disabled(disablesSingleReadingActions)
        }

        Divider()

        Button("Delete", role: .destructive) {
            appState.requestDelete(row)
        }
        .keyboardShortcut(ShortcutCatalog.delete)
        .disabled(appState.isDeleting)
    }

    private var disablesSingleReadingActions: Bool {
        appState.selectedIDs.contains(row.id) && appState.selectedIDs.count > 1
    }
}
