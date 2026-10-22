// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Menu bar commands that operate on the currently selected item.
struct ArticleCommands: Commands {
    var appState: AppState
    @FocusedValue(\.detailNavigationActions) private var detailNavigationActions
    @FocusedValue(\.boardActions) private var boardActions

    var body: some Commands {
        CommandMenu("Item") {
            Button("Mark as Photo") {
                if let detailNavigationActions {
                    detailNavigationActions.markAsPhoto()
                } else {
                    boardActions?.markAsPhoto()
                }
            }
            .keyboardShortcut(ShortcutCatalog.markAsPhoto)
            .disabled(!canClassifyImages)

            Button("Mark as Graphic") {
                if let detailNavigationActions {
                    detailNavigationActions.markAsGraphic()
                } else {
                    boardActions?.markAsGraphic()
                }
            }
            .keyboardShortcut(ShortcutCatalog.markAsGraphic)
            .disabled(!canClassifyImages)

            Divider()

            if !selectedRows.isEmpty {
                Button("Open Detail") {
                    boardActions?.openSelection()
                }
                .keyboardShortcut(ShortcutCatalog.open)
                .disabled(boardActions?.canOpenSelection != true || appState.isEditingText)

                Button("Quick Look") {
                    boardActions?.toggleQuickLook()
                }
                .disabled(boardActions?.canQuickLookSelection != true || appState.isEditingText)

                Divider()

                if let url = selectedRow?.sourceURL {
                    Button("Open Source") {
                        ReadingLink.open(url)
                    }
                    .keyboardShortcut(ShortcutCatalog.openSource)

                    Button("Copy Source URL") {
                        if let selectedRow {
                            ReadingClipboard.copySourceURL(selectedRow)
                        }
                    }
                    .keyboardShortcut(ShortcutCatalog.copySourceURL)
                }

                Divider()

                Button("Copy") {
                    if let selectedRow {
                        ReadingClipboard.copyFile(selectedRow, libraryURL: appState.libraryURL)
                    }
                }
                .keyboardShortcut(ShortcutCatalog.copy)
                .disabled(!canCopySelection)

                Divider()

                Button(detailNavigationActions == nil ? "Add Tag…" : "Focus Tag Field") {
                    boardActions?.editTags()
                }
                .keyboardShortcut(ShortcutCatalog.editTags)
                // Tag editing has no conflicting text-editing key equivalent;
                // it must remain available while search or a tag field is focused.
                .disabled(boardActions == nil || appState.isDeleting)

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
                .disabled(appState.isEditingText || appState.isBrowsingWebContent || appState.isDeleting)

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
        guard let selectedRow, !appState.isEditingText,
              boardActions != nil else { return false }
        return ReadingClipboard.fileURL(for: selectedRow, libraryURL: appState.libraryURL) != nil
    }

    private var canClassifyImages: Bool {
        guard !appState.isEditingText, !appState.isBrowsingWebContent,
              !appState.isDeleting else { return false }
        return detailNavigationActions?.canClassifyImages ?? boardActions?.canClassifyImages ?? false
    }
}
