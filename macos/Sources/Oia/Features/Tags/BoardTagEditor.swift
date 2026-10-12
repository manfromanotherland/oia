// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Adds user Tags to a fixed snapshot of cards while allowing several names
/// to be entered without reopening the board's tag editor.
struct BoardTagEditor: View {
    let rows: [ReadingRow]
    let librarySession: UInt64

    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var isAdding = false
    @State private var feedback: String?
    @State private var addedTags: [String] = []
    @FocusState private var isFocused: Bool

    private var targetIDs: [String] {
        rows.map(\.id)
    }

    private var trimmedQuery: String {
        query.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// The selection can change while a write refreshes the board. Resolve only
    /// the captured targets, retaining their snapshots if a filter hides them.
    private var currentRows: [ReadingRow] {
        let current = Dictionary(uniqueKeysWithValues: appState.readings.map { ($0.id, $0) })
        return rows.map { current[$0.id] ?? $0 }
    }

    private var commonUserTags: [String] {
        let targets = currentRows
        guard let first = targets.first else { return [] }
        return first.tags.filter { tag in
            targets.allSatisfy { row in
                row.tags.contains { ExactTagIdentity.matches($0, tag) }
                    && !row.machineTags.contains { ExactTagIdentity.matches($0, tag) }
            }
        }
    }

    private var suggestions: [BoardSearchToken] {
        BoardSearchSuggestions(
            text: query,
            tagCandidates: appState.filters.searchTagCandidates,
            selectedTokens: commonUserTags.map { BoardSearchToken(kind: .tag, value: $0) },
            includeVisualToken: false
        ).tagTokens
    }

    private var lengthError: String? {
        guard !TagRules.isWithinLength(trimmedQuery) else { return nil }
        return "Tags can be at most \(TagRules.maxLength) characters."
    }

    private var canAdd: Bool {
        !rows.isEmpty && !trimmedQuery.isEmpty && lengthError == nil && !isAdding
            && librarySession == appState.librarySessionGeneration
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Add tags")
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)
                Text(rows.count == 1 ? "1 card selected" : "\(rows.count) cards selected")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            TextField("Add a tag…", text: $query)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                .focused($isFocused)
                .textInputSuggestions {
                    ForEach(suggestions) { token in
                        Label(token.displayValue, systemImage: "tag")
                            .textInputCompletion(token.value)
                    }
                }
                .onSubmit(addTag)
                .accessibilityLabel("Add a tag")
                .accessibilityHint(
                    "Type a tag or choose a suggestion, then press Return to add it to the selected cards."
                )
                .accessibilityIdentifier(A11y.BoardTags.input)

            Text("Existing tags will be kept.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if !addedTags.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Added tags")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    FlowLayout(spacing: 6) {
                        ForEach(addedTags, id: \.self) { tag in
                            InspectorTagSummaryPill(title: tag)
                                .accessibilityIdentifier(A11y.BoardTags.tag(tag))
                        }
                    }
                }
            }

            if let lengthError {
                Text(lengthError)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier(A11y.BoardTags.inputError)
            }

            if let feedback {
                Text(feedback)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier(A11y.BoardTags.feedback)
            }

            HStack {
                Button("Add Tag", action: addTag)
                    .disabled(!canAdd)
                    .accessibilityIdentifier(A11y.BoardTags.add)
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier(A11y.BoardTags.done)
            }
        }
        .padding(16)
        .frame(width: 320)
        .accessibilityIdentifier(A11y.BoardTags.editor)
        .onExitCommand { dismiss() }
        .task { isFocused = true }
    }

    private func addTag() {
        guard canAdd else { return }
        let tag = trimmedQuery
        let ids = targetIDs
        query = ""
        feedback = nil
        isAdding = true
        isFocused = true
        Task {
            let addedCount = await appState.addTag(tag, to: ids)
            if addedCount == 0 {
                feedback = "Unable to add “\(tag)”. Try again."
                if query.isEmpty {
                    query = tag
                }
            } else if addedCount < ids.count {
                feedback = "Added “\(tag)” to \(addedCount) of \(ids.count) cards."
            } else {
                if !addedTags.contains(where: { ExactTagIdentity.matches($0, tag) }) {
                    addedTags.append(tag)
                }
                feedback = ids.count == 1
                    ? "Added “\(tag)” to 1 card."
                    : "Added “\(tag)” to \(addedCount) cards."
            }
            isAdding = false
        }
    }
}
