// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A reading-list query in app language: the composed kind/scope/tag filter,
/// optional free text and scoped search terms, the sort, and result bounds.
/// `CoreBridge` turns it into the core's compatible `FfiListOptions` at the
/// bridge boundary.
struct ReadingQuery {
    var kind: ReadingKind?
    var scope: LibraryScope
    var sort: ReadingSort
    var ascending: Bool
    var tag: String?
    var search: String?
    /// Exact saved tags that must all belong to the same reading.
    var tagTerms: [String]
    /// Derived labels/colours that must all belong to the same reading's
    /// current visual analysis.
    var visualTerms: [String]
    /// Exact palette colors, combined with the other terms by AND.
    var colorTerms: [String]
    var itemTypeTerms: [String]
    /// Core Spotlight's best-first semantic matches for `search`. The Rust
    /// core merges these candidates with its own text/label/colour results so
    /// filters and relevance ordering stay one coherent query.
    var semanticCandidateIDs: [String]
    /// Core Spotlight matches for the structured visual terms. Keeping these
    /// separate prevents a free-text metadata hit from satisfying an
    /// "In this image" token.
    var visualSemanticCandidateIDs: [String]
    var limit: UInt32
    var offset: UInt32
}

/// Intersects per-term Spotlight rankings without losing the stable order of
/// the first term. A reading must be recognised for every selected visual term,
/// so `[blue] [furniture]` can never combine evidence from different images.
enum VisualSemanticCandidateIntersection {
    static func ranked(_ candidateSets: [[String]]) -> [String] {
        guard let first = candidateSets.first else { return [] }
        let remaining = candidateSets.dropFirst().map(Set.init)
        var seen = Set<String>()
        return first.filter { candidate in
            !candidate.isEmpty
                && seen.insert(candidate).inserted
                && remaining.allSatisfy { $0.contains(candidate) }
        }
    }
}

extension ReadingQuery {
    /// The board always composes its selected scope and optional search into
    /// one core query. Search changes ordering, never the active board scope.
    static func boardSnapshot(
        scope: LibraryScope,
        search: String?,
        tagTerms: [String],
        visualTerms: [String],
        colorTerms: [String] = [],
        itemTypeTerms: [String] = [],
        semanticCandidateIDs: [String],
        visualSemanticCandidateIDs: [String]
    ) -> Self {
        let isSearching = search != nil || !tagTerms.isEmpty || !visualTerms.isEmpty || !colorTerms.isEmpty || !itemTypeTerms.isEmpty
        return Self(
            kind: scope.readingKindFilter,
            scope: scope,
            sort: isSearching ? .relevance : .savedAt,
            ascending: false,
            tag: nil,
            search: search,
            tagTerms: tagTerms,
            visualTerms: visualTerms,
            colorTerms: colorTerms,
            itemTypeTerms: itemTypeTerms,
            semanticCandidateIDs: semanticCandidateIDs,
            visualSemanticCandidateIDs: visualSemanticCandidateIDs,
            limit: .max,
            offset: 0
        )
    }
}

/// Delivers one captured search generation. The native client supplies its
/// generation guard; the Rust query owns filtering and combined relevance.
enum ReadingSnapshotDelivery {
    struct Candidates: Equatable, Sendable {
        var text: [String] = []
        var visual: [String] = []

        static let empty = Candidates()

        var isEmpty: Bool {
            text.isEmpty && visual.isEmpty
        }
    }

    @MainActor
    static func load<Rows>(
        textFirst: Bool = true,
        fetch: @MainActor (Candidates) async throws -> Rows,
        semanticCandidates: (@MainActor () async throws -> Candidates)?,
        isCurrent: @MainActor () -> Bool,
        publish: @MainActor (Rows, Bool) -> Void
    ) async throws -> Bool {
        guard isCurrent() else { return false }
        // Reconciliation of an already displayed semantic query keeps its
        // membership stable until one complete replacement is available.
        if !textFirst, let semanticCandidates {
            guard let candidates = await optionalCandidates(semanticCandidates),
                  isCurrent() else { return false }
            let rows = try await fetch(candidates)
            guard isCurrent() else { return false }
            publish(rows, !candidates.isEmpty)
            return true
        }
        let rows = try await fetch(.empty)
        guard isCurrent() else { return false }
        publish(rows, false)
        guard let semanticCandidates, isCurrent() else { return isCurrent() }

        guard let candidates = await optionalCandidates(semanticCandidates),
              isCurrent() else { return false }
        guard !candidates.isEmpty else { return true }

        let enriched = try await fetch(candidates)
        guard isCurrent() else { return false }
        publish(enriched, true)
        return true
    }

    @MainActor
    private static func optionalCandidates(
        _ load: @MainActor () async throws -> Candidates
    ) async -> Candidates? {
        do {
            return try await load()
        } catch is CancellationError {
            return nil
        } catch {
            // Spotlight is optional. Its failure must not hide local results.
            return .empty
        }
    }
}
