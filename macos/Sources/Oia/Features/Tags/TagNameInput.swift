// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Shared name entry for the Inspector and board tag editor. Suggestions are
/// buttons that complete the bound field; Return submits through the same action
/// as the editor's Add Tag button.
struct TagNameInput: View {
    @Binding var query: String
    let candidates: [BoardSearchTagCandidate]
    let userTags: [String]
    let isFocused: FocusState<Bool>.Binding
    let accessibilityIdentifier: String
    let errorAccessibilityIdentifier: String
    var accessibilityHint = "Type a tag or choose a suggestion, then press Return to add it."
    var onSubmit: () -> Void

    @State private var completedQuery: String?

    private var suggestions: [BoardSearchToken] {
        guard query != completedQuery else { return [] }
        return BoardSearchSuggestions(
            text: query,
            tagCandidates: candidates,
            selectedTokens: userTags.map { BoardSearchToken(kind: .tag, value: $0) },
            includeVisualToken: false
        ).tagTokens
    }

    private var lengthError: String? {
        let tag = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !TagRules.isWithinLength(tag) else { return nil }
        return "Tags can be at most \(TagRules.maxLength) characters."
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            TextField("Add a tag…", text: $query)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                .focused(isFocused)
                .onSubmit(onSubmit)
                .accessibilityLabel("Add a tag")
                .accessibilityHint(accessibilityHint)
                .accessibilityIdentifier(accessibilityIdentifier)

            // Keep suggestions independent of field focus so a focus change
            // during a click cannot remove its target before the action runs.
            // Each button assigns the parent's query before restoring focus.
            if !suggestions.isEmpty {
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(suggestions) { token in
                            Button {
                                completedQuery = token.value
                                query = token.value
                                isFocused.wrappedValue = true
                            } label: {
                                Label(token.displayValue, systemImage: "tag")
                                    .font(.callout)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 4)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(token.displayValue)
                            .accessibilityHint("Use this tag name, then press Return to add it.")
                            .accessibilityIdentifier("\(accessibilityIdentifier).suggestion.\(token.value)")
                        }
                    }
                }
                .frame(height: min(CGFloat(suggestions.count) * 28, 112))
                .background(.quaternary, in: RoundedRectangle(cornerRadius: 6))
            }

            if let lengthError {
                Text(lengthError)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier(errorAccessibilityIdentifier)
            }
        }
        .onChange(of: query) { _, newValue in
            if newValue != completedQuery {
                completedQuery = nil
            }
        }
    }
}
