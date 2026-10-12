// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct TagInputFocusRequest: Equatable {
    let readingID: String
    let generation: UInt64
}

/// Tag suggestions complete the name; Return adds it as a user Tag.
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

    var body: some View {
        TagNameInput(
            query: $query,
            candidates: candidates,
            userTags: userTags,
            isFocused: $isFocused,
            accessibilityIdentifier: A11y.Inspector.tagInput,
            errorAccessibilityIdentifier: A11y.Inspector.tagInputError,
            onSubmit: addTag
        )
        .onExitCommand {
            query = ""
            isFocused = false
        }
        .task(id: focusRequest) {
            if focusRequest?.readingID == readingID {
                isFocused = true
            }
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
