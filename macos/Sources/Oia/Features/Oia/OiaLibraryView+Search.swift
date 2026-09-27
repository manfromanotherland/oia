// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

extension OiaLibraryView {
    private var searchSuggestions: BoardSearchSuggestions {
        BoardSearchSuggestions(
            text: appState.searchQuery,
            tagCandidates: appState.filters.searchTagCandidates,
            selectedTokens: appState.searchTokens,
            includeVisualToken: appState.hasAvailableVisualSearchSuggestion
        )
    }

    @ViewBuilder
    var nativeSearchSuggestions: some View {
        let suggestions = searchSuggestions
        if !suggestions.tagTokens.isEmpty {
            Section("Tags") {
                ForEach(suggestions.tagTokens) { token in
                    Text(token.displayValue)
                        .searchCompletion(token)
                }
            }
        }

        if let token = suggestions.visualToken {
            Section("In this image") {
                Text(token.displayValue)
                    .searchCompletion(token)
            }
        }
    }
}
