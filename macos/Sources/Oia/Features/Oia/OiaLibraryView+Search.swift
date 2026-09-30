// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

extension OiaLibraryView {
    private var searchSuggestions: BoardSearchSuggestions {
        BoardSearchSuggestions(
            text: appState.searchQuery,
            tagCandidates: appState.filters.searchTagCandidates,
            selectedTokens: appState.searchTokens,
            includeVisualToken: false
        )
    }

    @ViewBuilder
    var nativeSearchSuggestions: some View {
        let suggestions = searchSuggestions
        if !suggestions.itemTypeTokens.isEmpty {
            Section("Item type") {
                ForEach(suggestions.itemTypeTokens) { token in
                    Text(token.displayValue.capitalized)
                        .searchCompletion(token)
                }
            }
        }
        if let token = suggestions.colorToken {
            Section("Color") {
                Text(token.displayValue)
                    .searchCompletion(token)
            }
        }
        if !suggestions.tagTokens.isEmpty {
            Section("Tags") {
                ForEach(suggestions.tagTokens) { token in
                    Text(token.displayValue)
                        .searchCompletion(token)
                }
            }
        }
    }
}
