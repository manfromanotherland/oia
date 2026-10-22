// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Actions in a card's context menu. Mutations still flow through `AppState`.
struct OiaReadingActions: View {
    @Environment(AppState.self) private var appState

    let row: ReadingRow
    var onAddTag: () -> Void

    var body: some View {
        Button("Copy") {
            ReadingClipboard.copyFile(row, libraryURL: appState.libraryURL)
        }
        .keyboardShortcut(ShortcutCatalog.copy)
        .disabled(ReadingClipboard.fileURL(for: row, libraryURL: appState.libraryURL) == nil)

        Button("Copy Source URL") {
            ReadingClipboard.copySourceURL(row)
        }
        .keyboardShortcut(ShortcutCatalog.copySourceURL)
        .disabled(row.sourceURL == nil || disablesSingleReadingActions)

        Divider()

        Button("Add Tag…") { onAddTag() }
            .keyboardShortcut(ShortcutCatalog.editTags)
            .disabled(appState.isDeleting)

        if row.kind == .image {
            Menu("Photography") {
                Button("Include in Photography") {
                    appState.enqueueImageTypeEdit(.photograph, ids: imageTypeTargetIDs)
                }
                Button("Hide from Photography") {
                    appState.enqueueImageTypeEdit(.other, ids: imageTypeTargetIDs)
                }
                Divider()
                Button("Use Automatic Classification") {
                    appState.enqueueImageTypeEdit(nil, ids: imageTypeTargetIDs)
                }
            }
            .disabled(appState.isDeleting)
        }

        if let url = row.sourceURL {
            Button("Open Source") {
                ReadingLink.open(url)
            }
            .keyboardShortcut(ShortcutCatalog.openSource)
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

    private var imageTypeTargetIDs: [String] {
        if appState.selectedIDs.contains(row.id), appState.selectedIDs.count > 1 {
            appState.selectedRows.filter { $0.kind == .image }.map(\.id)
        } else {
            [row.id]
        }
    }
}
