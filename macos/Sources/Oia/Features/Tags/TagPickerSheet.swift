// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Modal tag picker for an article. A search field filters the library's tags;
/// with no query it lists the article's tags first, then others. Clicking a tag
/// toggles it on or off for the article (a checkmark marks applied tags). Typing
/// a name that doesn't exist yet offers an "Add" row — or press Return — to
/// create and apply it.
///
/// Changes apply live through `onToggle`, so the sheet stays open to manage
/// several tags in a row and closing needs no explicit save.
struct TagPickerSheet: View {
    /// Tags currently applied to the article. Re-supplied by the presenter on
    /// every render, so checkmarks stay in step as toggles take effect.
    var applied: [String]
    /// All library tag names, alphabetical (the order of `LibraryFilters.tags`).
    /// The sheet re-sorts anyway — applied tags float to the top on open.
    var allTags: [String]
    var machineTags: [String]
    /// `(tag, shouldApply)` — apply the tag when `true`, remove it when `false`.
    var onToggle: (String, Bool) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @FocusState private var searchFocused: Bool
    /// Display order, snapshotted when the sheet opens (applied tags first, then
    /// the rest) and held stable while open, so toggling a tag never reorders the
    /// list. Seeded in `init`, so it's correct on the first frame; re-derived each
    /// time the sheet reopens (fresh @State). Checkmarks track the live `applied`.
    @State private var order: [String]

    init(applied: [String], allTags: [String], machineTags: [String] = [], onToggle: @escaping (String, Bool) -> Void) {
        self.applied = applied
        self.allTags = allTags
        self.machineTags = machineTags
        self.onToggle = onToggle
        // Freeze the order at presentation: applied tags first, then the rest.
        let appliedSet = Set(applied.map { ExactTagIdentity.bytes($0) })
        _order = State(initialValue: applied + allTags.filter { !appliedSet.contains(ExactTagIdentity.bytes($0)) })
    }

    private var trimmedQuery: String {
        query.trimmingCharacters(in: .whitespaces)
    }

    private var appliedSet: Set<Data> {
        Set(applied.map { ExactTagIdentity.bytes($0) })
    }

    /// The list contents, drawn from the frozen `order`: idle shows the first ten,
    /// searching shows every match. Applied tags already sit first in `order`.
    private var listed: [String] {
        let needle = trimmedQuery.lowercased()
        if needle.isEmpty {
            return Array(order.prefix(10))
        }
        return order.filter { $0.lowercased().contains(needle) }
    }

    /// The tag to offer creating — the typed name, when it doesn't already exist
    /// (case-insensitive) and fits within the length limit. `nil` hides the
    /// "Add" row (an over-length name shows `lengthError` instead).
    private var creatable: String? {
        let typed = trimmedQuery
        guard !typed.isEmpty,
              TagRules.isWithinLength(typed),
              !allTags.contains(where: { ExactTagIdentity.matches($0, typed) })
        else { return nil }
        return typed
    }

    /// Inline error for a typed name that's too long to create, or `nil` when
    /// the name fits. An over-length name can never match an existing tag (they
    /// were all created under the same limit), so this only gates creation.
    private var lengthError: String? {
        let typed = trimmedQuery
        guard !typed.isEmpty, !TagRules.isWithinLength(typed) else { return nil }
        return "Tags can be at most \(TagRules.maxLength) characters."
    }

    var body: some View {
        VStack(spacing: 0) {
            searchField
            if let lengthError {
                Text(lengthError)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.bottom, 8)
                    .accessibilityIdentifier(A11y.TagPicker.lengthError)
            }
            Divider()
            tagList
            Divider()
            HStack {
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier(A11y.TagPicker.done)
            }
            .padding(12)
        }
        .frame(width: 380, height: 460)
        // Esc closes the sheet even while the search field holds focus — a
        // focused TextField can swallow the Done button's cancel shortcut.
        .onExitCommand { dismiss() }
    }

    private var searchField: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)
            TextField("Search or add tags…", text: $query)
                .textFieldStyle(.plain)
                .focused($searchFocused)
                .onSubmit(submit)
                .accessibilityIdentifier(A11y.TagPicker.searchField)
            if !query.isEmpty {
                Button { query = "" } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(12)
        .onAppear { searchFocused = true }
    }

    private var tagList: some View {
        List {
            if let creatable {
                Button { apply(creatable) } label: {
                    Label("Add “\(creatable)”", systemImage: "plus.circle")
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier(A11y.TagPicker.addRow)
            }
            ForEach(listed, id: \.self) { tag in
                Button { toggle(tag) } label: {
                    HStack {
                        let isMachine = machineTags.contains { ExactTagIdentity.matches($0, tag) }
                        if isMachine { Image(systemName: "sparkles") }
                        Text("#\(tag)")
                        Spacer()
                        if appliedSet.contains(ExactTagIdentity.bytes(tag)) {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.tint)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier(A11y.TagPicker.row(tag))
            }
            if listed.isEmpty, creatable == nil {
                Text(allTags.isEmpty
                    ? "No tags yet — type to create one."
                    : "No matching tags.")
                    .foregroundStyle(.secondary)
            }
        }
        .listStyle(.inset)
    }

    private func toggle(_ tag: String) {
        onToggle(tag, !appliedSet.contains(ExactTagIdentity.bytes(tag)))
    }

    private func apply(_ tag: String) {
        // A freshly created tag isn't in the frozen order yet — surface it at the
        // top so it's visible (and within the idle first-ten).
        if !order.contains(tag) {
            order.insert(tag, at: 0)
        }
        onToggle(tag, true)
        query = ""
    }

    /// Return key: apply an existing exact match (case-insensitive) if there is
    /// one, otherwise create and apply the typed tag.
    private func submit() {
        let typed = trimmedQuery
        guard !typed.isEmpty else { return }
        if let existing = allTags.first(where: { ExactTagIdentity.matches($0, typed) }) {
            if !appliedSet.contains(ExactTagIdentity.bytes(existing))
                || machineTags.contains(where: { ExactTagIdentity.matches($0, existing) }) {
                onToggle(typed, true)
            }
        } else {
            // Block creating an over-length tag; keep the text so the inline
            // error stays visible for the user to trim it down.
            guard TagRules.isWithinLength(typed) else { return }
            if !order.contains(typed) {
                order.insert(typed, at: 0)
            }
            onToggle(typed, true)
        }
        query = ""
    }
}
