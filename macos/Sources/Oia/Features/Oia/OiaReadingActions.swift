// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// One action vocabulary used by both a card's context menu and its restrained
/// hover ellipsis. Mutations still flow through `AppState`.
struct OiaReadingActions: View {
    @Environment(AppState.self) private var appState

    let row: ReadingRow
    var onEditTags: () -> Void

    var body: some View {
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
