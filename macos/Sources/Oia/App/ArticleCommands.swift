// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Menu bar commands that operate on the currently selected item.
struct ArticleCommands: Commands {
    var appState: AppState
    @FocusedValue(\.detailNavigationActions) private var detailNavigationActions
    @FocusedValue(\.boardActions) private var boardActions

    var body: some Commands {
        CommandMenu("Item") {
            if !selectedRows.isEmpty {
                Button("Open") {
                    boardActions?.openSelection()
                }
                .keyboardShortcut(ShortcutCatalog.open)
                .disabled(boardActions?.canOpenSelection != true || appState.isEditingText)

                Button("Quick Look") {
                    boardActions?.toggleQuickLook()
                }
                .disabled(boardActions?.canQuickLookSelection != true || appState.isEditingText)

                Divider()

                Button("Copy") {
                    if let selectedRow {
                        ReadingClipboard.copyFile(selectedRow, libraryURL: appState.libraryURL)
                    }
                }
                .keyboardShortcut(ShortcutCatalog.copy)
                .disabled(!canCopySelection)

                Button("Copy Address") {
                    if let selectedRow {
                        ReadingClipboard.copyAddress(selectedRow, libraryURL: appState.libraryURL)
                    }
                }
                .keyboardShortcut(ShortcutCatalog.copyAddress)
                .disabled(!canCopySelection)

                Divider()

                Button("Edit Tags…") {
                    appState.showTagSheet = true
                }
                .keyboardShortcut(ShortcutCatalog.editTags)
                .disabled(selectedRows.count != 1 || appState.isDeleting)

                Button(appState.showHighlights ? "Hide Highlights" : "Show Highlights") {
                    appState.showHighlights.toggle()
                }
                .keyboardShortcut(ShortcutCatalog.toggleHighlights)
                .disabled(
                    selectedRow?.kind != .article
                        || detailNavigationActions == nil
                )

                Divider()

                Button(deleteTitle, role: .destructive) {
                    appState.requestDeleteSelection()
                }
                .keyboardShortcut(ShortcutCatalog.delete)
                .disabled(appState.isEditingText || appState.isDeleting)

                Divider()

                if let url = selectedRow?.sourceURL {
                    Button("Open in Browser") {
                        ReadingLink.open(url)
                    }
                    .keyboardShortcut(ShortcutCatalog.openInBrowser)
                }
            } else {
                Text("No item selected")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var selectedRows: [ReadingRow] {
        appState.selectedRows
    }

    private var selectedRow: ReadingRow? {
        selectedRows.count == 1 ? selectedRows.first : nil
    }

    private var deleteTitle: String {
        selectedRows.count == 1 ? "Delete" : "Delete \(selectedRows.count) Items"
    }

    private var canCopySelection: Bool {
        guard let selectedRow, !appState.isEditingText, boardActions != nil else { return false }
        return ReadingClipboard.fileURL(for: selectedRow, libraryURL: appState.libraryURL) != nil
    }
}
