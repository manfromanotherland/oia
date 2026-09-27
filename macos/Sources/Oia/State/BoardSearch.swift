// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A completed, scoped term in the board's native search field.
///
/// The visible token is deliberately only `value`; `kind` records which index
/// field the core must search without putting implementation language in the UI.
struct BoardSearchToken: Identifiable, Hashable, Codable, Sendable {
    enum Kind: String, Codable, Sendable {
        case tag
        case visual
        case color
    }

    /// Stable SwiftUI identity derived from search meaning rather than creation
    /// time. The kind is part of the identity so equal text can appear once in
    /// each suggestion group.
    struct Identifier: Hashable, Codable, Sendable {
        let kind: Kind
        let encodedValue: Data
    }

    let kind: Kind
    let value: String

    var id: Identifier {
        Identifier(
            kind: kind,
            encodedValue: ExactTagIdentity.bytes(identityValue)
        )
    }

    /// Text shown inside the native search token. Exact tag bytes remain in
    /// `value` for the core predicate even when an externally-authored tag has
    /// surrounding whitespace or decomposed Unicode.
    var displayValue: String {
        kind == .color ? value : BoardSearchNormalization.value(value)
    }

    init(kind: Kind, value: String) {
        self.kind = kind
        switch kind {
        case .tag:
            self.value = value
        case .visual:
            self.value = BoardSearchNormalization.value(value)
        case .color:
            let hex = BoardSearchNormalization.value(value).split(separator: ":").last.map(String.init) ?? ""
            self.value = hex.count == 7 && hex.first == "#"
                && UInt64(hex.dropFirst(), radix: 16) != nil ? hex.uppercased() : ""
        }
    }

    private var identityValue: String {
        kind == .tag ? value : BoardSearchNormalization.value(value)
    }

    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.id == rhs.id
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(id)
    }

    private enum CodingKeys: String, CodingKey {
        case kind
        case value
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        try self.init(
            kind: container.decode(Kind.self, forKey: .kind),
            value: container.decode(String.self, forKey: .value)
        )
    }

    func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(kind, forKey: .kind)
        try container.encode(value, forKey: .value)
    }
}

/// Normalized structured terms for one board query.
///
/// Token order is retained for display and request construction, while equality
/// and hashing use a canonical identity because the terms combine by AND and
/// therefore have no semantic order.
struct BoardSearchCriteria: Hashable, Sendable {
    let tokens: [BoardSearchToken]

    init(tokens: [BoardSearchToken]) {
        var seen = Set<BoardSearchToken.Identifier>()
        self.tokens = tokens.compactMap { token in
            let normalized = BoardSearchToken(kind: token.kind, value: token.value)
            guard !normalized.displayValue.isEmpty,
                  seen.insert(normalized.id).inserted
            else {
                return nil
            }
            return normalized
        }
    }

    var tagTerms: [String] {
        tokens.compactMap { $0.kind == .tag ? $0.value : nil }
    }

    var visualTerms: [String] {
        tokens.compactMap { $0.kind == .visual ? $0.value : nil }
    }

    var colorTerms: [String] {
        tokens.compactMap { $0.kind == .color ? $0.value : nil }
    }

    var isActive: Bool {
        !tokens.isEmpty
    }

    var hasVisualTerms: Bool {
        tokens.contains { $0.kind == .visual }
    }

    /// Canonical identity for snapshots and generation checks. Reordering the
    /// same AND terms does not create a new search context.
    var semanticIdentity: [BoardSearchToken.Identifier] {
        tokens.map(\.id).sorted { lhs, rhs in
            if lhs.kind.rawValue != rhs.kind.rawValue {
                return lhs.kind.rawValue < rhs.kind.rawValue
            }
            return lhs.encodedValue.lexicographicallyPrecedes(rhs.encodedValue)
        }
    }

    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.semanticIdentity == rhs.semanticIdentity
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(semanticIdentity)
    }
}

/// Immutable identity for the complete search currently driving the board.
struct BoardSearchInput: Hashable, Sendable {
    let text: String?
    let criteria: BoardSearchCriteria

    init(text: String, tokens: [BoardSearchToken]) {
        let normalizedText = BoardSearchNormalization.value(text)
        self.text = normalizedText.isEmpty ? nil : normalizedText
        criteria = BoardSearchCriteria(tokens: tokens)
    }

    var isActive: Bool {
        text != nil || criteria.isActive
    }

    var hasVisualTerms: Bool {
        criteria.hasVisualTerms
    }
}

/// One cached tag-search candidate from the exact file-backed vocabulary.
///
/// Folding thousands of tag strings on every search-field keystroke made the
/// native completion menu's work unbounded. The global vocabulary changes only
/// after a library refresh, so compute its matching keys at that boundary.
struct BoardSearchTagCandidate: Equatable, Sendable {
    let token: BoardSearchToken
    let id: BoardSearchToken.Identifier
    let matchingKey: String

    init?(value: String) {
        let token = BoardSearchToken(kind: .tag, value: value)
        guard !token.displayValue.isEmpty else { return nil }
        self.token = token
        id = token.id
        matchingKey = BoardSearchNormalization.matchingKey(token.displayValue)
    }
}

/// Bounded native-search completions for the user's current unfinished text.
struct BoardSearchSuggestions: Equatable, Sendable {
    static let maximumTagCount = 8

    let tagTokens: [BoardSearchToken]
    let visualToken: BoardSearchToken?

    init(
        text: String,
        tagCandidates: [BoardSearchTagCandidate],
        selectedTokens: [BoardSearchToken],
        includeVisualToken: Bool = true
    ) {
        let value = BoardSearchNormalization.value(text)
        guard !value.isEmpty else {
            tagTokens = []
            visualToken = nil
            return
        }

        let selectedIDs = Set(BoardSearchCriteria(tokens: selectedTokens).semanticIdentity)
        let needle = BoardSearchNormalization.matchingKey(value)
        var prefixes: [BoardSearchToken] = []

        for candidate in tagCandidates where
            candidate.matchingKey.hasPrefix(needle)
            && !selectedIDs.contains(candidate.id)
        {
            prefixes.append(candidate.token)
            if prefixes.count == Self.maximumTagCount {
                break
            }
        }

        var matches = prefixes
        if matches.count < Self.maximumTagCount {
            for candidate in tagCandidates where
                !candidate.matchingKey.hasPrefix(needle)
                && candidate.matchingKey.contains(needle)
                && !selectedIDs.contains(candidate.id)
            {
                matches.append(candidate.token)
                if matches.count == Self.maximumTagCount {
                    break
                }
            }
        }
        tagTokens = matches

        let visual = BoardSearchToken(kind: .visual, value: value)
        visualToken = includeVisualToken && !selectedIDs.contains(visual.id) ? visual : nil
    }
}

private enum BoardSearchNormalization {
    private static let matchingLocale = Locale(identifier: "en_US_POSIX")

    static func value(_ rawValue: String) -> String {
        rawValue
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .precomposedStringWithCanonicalMapping
    }

    static func matchingKey(_ value: String) -> String {
        value
            .folding(
                options: [.diacriticInsensitive, .widthInsensitive],
                locale: matchingLocale
            )
            .lowercased(with: matchingLocale)
    }
}
