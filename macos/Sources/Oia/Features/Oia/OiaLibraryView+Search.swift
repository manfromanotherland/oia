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

    private var isToolbarSearchExpanded: Bool {
        isSearchPresented || searchFocused
            || !appState.searchQuery.isEmpty
            || !appState.searchTokens.isEmpty
    }

    private var hasSearchSuggestions: Bool {
        let suggestions = searchSuggestions
        return !suggestions.tagTokens.isEmpty || suggestions.visualToken != nil
    }

    var toolbarSearchControl: some View {
        @Bindable var state = appState
        let expanded = isToolbarSearchExpanded

        return HStack(spacing: 6) {
            if expanded {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                    .frame(width: 16)
            } else {
                Button(action: focusSearch) {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                        .frame(width: 16)
                }
                .buttonStyle(.plain)
                .help("Search (⌘F)")
                .accessibilityLabel("Search")
            }

            if expanded {
                if !state.searchTokens.isEmpty {
                    ScrollView(.horizontal) {
                        HStack(spacing: 4) {
                            ForEach(state.searchTokens) { token in
                                searchTokenChip(token)
                            }
                        }
                    }
                    .scrollIndicators(.hidden)
                    .frame(maxWidth: 110)
                }

                TextField("Search Óia", text: $state.searchQuery)
                    .textFieldStyle(.plain)
                    .focused($searchFocused)
                    .frame(minWidth: 60)
                    .accessibilityIdentifier(A11y.Search.field)
                    .onSubmit { showSearchSuggestions = false }
                    .onExitCommand(perform: dismissToolbarSearch)

                if !state.searchQuery.isEmpty || !state.searchTokens.isEmpty {
                    Button(action: dismissToolbarSearch) {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .help("Clear search")
                    .accessibilityLabel("Clear search")
                }
            }
        }
        .padding(.leading, 10)
        .padding(.trailing, expanded ? 8 : 10)
        .frame(width: expanded ? 260 : 36, height: 34, alignment: .leading)
        .background(.regularMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(.primary.opacity(0.12)))
        .clipped()
        .animation(.easeInOut(duration: 0.22), value: expanded)
        .popover(isPresented: $showSearchSuggestions, arrowEdge: .bottom) {
            searchSuggestionsPopover
        }
        .onChange(of: state.searchQuery) { _, query in
            showSearchSuggestions = searchFocused && !query.isEmpty && hasSearchSuggestions
        }
        .onChange(of: searchFocused) { _, focused in
            if focused {
                showSearchSuggestions = !state.searchQuery.isEmpty && hasSearchSuggestions
            } else if state.searchQuery.isEmpty && state.searchTokens.isEmpty {
                withAnimation(.easeInOut(duration: 0.22)) {
                    isSearchPresented = false
                }
            }
        }
        .onChange(of: state.searchQuery.isEmpty && state.searchTokens.isEmpty) { _, empty in
            if empty && !searchFocused {
                withAnimation(.easeInOut(duration: 0.22)) {
                    isSearchPresented = false
                }
            }
        }
        .onChange(of: searchSuggestions) { _, _ in
            if searchFocused {
                showSearchSuggestions = hasSearchSuggestions
            }
        }
    }

    @ViewBuilder
    private func searchTokenChip(_ token: BoardSearchToken) -> some View {
        let chip = HStack(spacing: 3) {
            Text(token.displayValue)
                .lineLimit(1)
            Button {
                appState.searchTokens.removeAll { $0.id == token.id }
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 9, weight: .semibold))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Remove \(token.displayValue)")
        }
        .font(.caption)
        .padding(.horizontal, 6)
        .padding(.vertical, 3)

        if token.kind == .color,
           let palette = CardThemePalette(themeColor: token.value) {
            chip
                .foregroundStyle(palette.foreground.color)
                .background(palette.background.color, in: Capsule())
        } else {
            chip.background(.quaternary, in: Capsule())
        }
    }

    private var searchSuggestionsPopover: some View {
        let suggestions = searchSuggestions
        return VStack(alignment: .leading, spacing: 4) {
            if !suggestions.tagTokens.isEmpty {
                Text("Tags")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 8)
                ForEach(suggestions.tagTokens) { token in
                    searchSuggestionButton(token)
                }
            }

            if let token = suggestions.visualToken {
                Text("In this image")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 8)
                    .padding(.top, suggestions.tagTokens.isEmpty ? 0 : 6)
                searchSuggestionButton(token)
            }
        }
        .padding(8)
        .frame(width: 240, alignment: .leading)
    }

    private func searchSuggestionButton(_ token: BoardSearchToken) -> some View {
        Button {
            appState.searchTokens = BoardSearchCriteria(
                tokens: appState.searchTokens + [token]
            ).tokens
            appState.searchQuery = ""
            showSearchSuggestions = false
            searchFocused = true
        } label: {
            Text(token.displayValue)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 8)
                .padding(.vertical, 5)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func dismissToolbarSearch() {
        appState.clearSearch()
        showSearchSuggestions = false
        searchFocused = false
        withAnimation(.easeInOut(duration: 0.22)) {
            isSearchPresented = false
        }
    }
}
