// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Edits a frozen board selection using the Inspector's tag controls.
struct BoardTagEditor: View {
    @Environment(AppState.self) private var appState
    let onClose: () -> Void
    @State private var editing: BoardTagEditingSession
    @State private var query = ""
    @State private var tagPendingRemoval: BoardTagEditorState.Tag?
    @State private var showsTagRemovalConfirmation = false
    @FocusState private var isFocused: Bool

    init(rows: [ReadingRow], librarySession: UInt64, onClose: @escaping () -> Void) {
        self.onClose = onClose
        // A new dialog request owns its target snapshots for its whole lifetime.
        _editing = State(initialValue: BoardTagEditingSession(rows: rows, librarySession: librarySession))
    }

    private var canAdd: Bool {
        let tag = query.trimmingCharacters(in: .whitespacesAndNewlines)
        return !tag.isEmpty && TagRules.isWithinLength(tag)
            && !editing.state.targetIDs.isEmpty
            && editing.state.librarySession == appState.librarySessionGeneration
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Add tags")
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                Button(action: close) {
                    Label("Close", systemImage: "xmark")
                        .labelStyle(.iconOnly)
                        .frame(width: 24, height: 24)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                .keyboardShortcut(.cancelAction)
                .accessibilityIdentifier(A11y.BoardTags.close)
            }

            TagNameInput(
                query: $query,
                candidates: appState.filters.searchTagCandidates,
                userTags: editing.state.commonUserTags,
                isFocused: $isFocused,
                accessibilityIdentifier: A11y.BoardTags.input,
                errorAccessibilityIdentifier: A11y.BoardTags.inputError,
                floatsSuggestions: true,
                onSubmit: addTag
            )
            .zIndex(1)

            if !editing.state.tags.isEmpty {
                ViewThatFits(in: .vertical) {
                    tagPills
                    ScrollView {
                        tagPills
                    }
                    .scrollBounceBehavior(.basedOnSize)
                    .frame(height: 240)
                }
                .frame(maxHeight: 240)
                .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(24)
        .frame(width: 352)
        .background(.background, in: RoundedRectangle(cornerRadius: 20))
        .shadow(color: .black.opacity(0.25), radius: 24, y: 8)
        .accessibilityIdentifier(A11y.BoardTags.editor)
        .defaultFocus($isFocused, true)
        .onExitCommand(perform: close)
        .task {
            editing.updateRows(appState.readings, librarySession: appState.librarySessionGeneration)
            await Task.yield()
            guard !Task.isCancelled else { return }
            isFocused = true
        }
        .onDisappear { isFocused = false }
        .onChange(of: appState.readings) { _, rows in
            editing.updateRows(rows, librarySession: appState.librarySessionGeneration)
        }
        .confirmationDialog(
            "Remove this tag?",
            isPresented: $showsTagRemovalConfirmation,
            presenting: tagPendingRemoval
        ) { tag in
            Button("Remove tag", role: .destructive) {
                editing.removeTag(tag.name, using: appState)
                tagPendingRemoval = nil
            }
            Button("Cancel", role: .cancel) { tagPendingRemoval = nil }
        } message: { tag in
            Text("“\(tag.name)” will be removed from \(removalTargets) and won’t be added back automatically.")
        }
    }

    private var tagPills: some View {
        FlowLayout(spacing: 6) {
            ForEach(editing.state.tags) { tag in
                tagPill(tag)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
    }

    private func close() {
        isFocused = false
        onClose()
    }

    private var removalTargets: String {
        editing.state.targetIDs.count == 1 ? "this item" : "the selected cards"
    }

    private func tagPill(_ tag: BoardTagEditorState.Tag) -> some View {
        InspectorTagPill(
            title: tag.name,
            symbol: tag.isMachine ? "sparkles" : "tag",
            removeAccessibilityIdentifier: A11y.BoardTags.removeTag(tag.name),
            onSearch: { search(tag.name) },
            onRemove: { requestRemoval(tag) }
        )
        .accessibilityLabel("\(tag.name), \(tag.isMachine ? "machine tag" : "your tag")")
        .accessibilityIdentifier(A11y.BoardTags.tag(tag.name))
        .contextMenu {
            if tag.isMachine {
                Button("Make this my tag", systemImage: "tag") {
                    editing.addTag(tag.name, using: appState)
                }
            }
            Button("Remove tag", systemImage: "xmark", role: .destructive) {
                requestRemoval(tag)
            }
        }
    }

    private func addTag() {
        guard canAdd else { return }
        let tag = query.trimmingCharacters(in: .whitespacesAndNewlines)
        editing.addTag(tag, using: appState)
        query = ""
        isFocused = true
    }

    private func requestRemoval(_ tag: BoardTagEditorState.Tag) {
        tagPendingRemoval = tag
        showsTagRemovalConfirmation = true
    }

    private func search(_ tag: String) {
        let nextTokens = [BoardSearchToken(kind: .tag, value: tag)]
        let searchChanged = !appState.searchQuery.isEmpty
            || BoardSearchCriteria(tokens: appState.searchTokens) != BoardSearchCriteria(tokens: nextTokens)
        appState.activeScope = .all
        appState.searchQuery = ""
        appState.searchTokens = nextTokens
        if !searchChanged {
            appState.searchDidChange()
        }
        close()
    }
}
