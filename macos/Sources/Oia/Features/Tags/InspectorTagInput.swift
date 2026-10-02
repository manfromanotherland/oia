// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct TagInputFocusRequest: Equatable {
    let readingID: String
    let generation: UInt64
}

/// Native text suggestions complete the name; Return adds it as a user Tag.
struct InspectorTagInput: View {
    let readingID: String
    let candidates: [BoardSearchTagCandidate]
    let userTags: [String]
    let focusRequest: TagInputFocusRequest?
    var onAdd: (String) -> Void

    @State private var query = ""
    @FocusState private var isFocused: Bool

    private var trimmedQuery: String {
        query.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var suggestions: [BoardSearchToken] {
        BoardSearchSuggestions(
            text: query,
            tagCandidates: candidates,
            selectedTokens: userTags.map { BoardSearchToken(kind: .tag, value: $0) },
            includeVisualToken: false
        ).tagTokens
    }

    private var lengthError: String? {
        guard !TagRules.isWithinLength(trimmedQuery) else { return nil }
        return "Tags can be at most \(TagRules.maxLength) characters."
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
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
                .onExitCommand {
                    query = ""
                    isFocused = false
                }
                .accessibilityLabel("Add a tag")
                .accessibilityHint("Type a tag or choose a suggestion, then press Return to add it.")
                .accessibilityIdentifier(A11y.Inspector.tagInput)

            if let lengthError {
                Text(lengthError)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier(A11y.Inspector.tagInputError)
            }
        }
        .task(id: focusRequest) {
            if focusRequest?.readingID == readingID { isFocused = true }
        }
        .onChange(of: readingID) { _, _ in
            query = ""
            isFocused = focusRequest?.readingID == readingID
        }
    }

    private func addTag() {
        let tag = trimmedQuery
        guard !tag.isEmpty, TagRules.isWithinLength(tag) else { return }
        onAdd(tag)
        query = ""
    }
}
