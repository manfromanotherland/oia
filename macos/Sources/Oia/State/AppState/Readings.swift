// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

struct BoardQueryContext: Hashable {
    let scope: LibraryScope
    let search: BoardSearchInput
    var imageTypeFilter: ImageTypeFilter = .all
}

/// One immutable board snapshot. Scope, free text, structured terms, and
/// Spotlight ranking remain coherent while the complete result is loaded.
private struct ReadingSnapshotContext {
    let generation: UInt64
    let scope: LibraryScope
    let search: BoardSearchInput
    let imageTypeFilter: ImageTypeFilter

    var boardContext: BoardQueryContext {
        BoardQueryContext(scope: scope, search: search, imageTypeFilter: imageTypeFilter)
    }
}

enum ReadingLoadResult: Equatable {
    case published
    case superseded
    case failed
}

// ── Reading list ─────────────────────────────────────────────────────────────
// Loading the list, debounced search, and tag metadata.

extension AppState {
    // ── Refresh (list + filters) ──────────────────────────────────────────

    func refresh() async {
        // Invalidate an older in-flight query before child tasks can yield; it
        // must not consume this content publication with pre-mutation rows.
        readingLoadGeneration &+= 1
        libraryContentRefreshPending = true
        await withTaskGroup(of: Void.self) { group in
            // A refresh follows a local mutation (or a watcher sync), so never
            // re-home a selection the mutation already advanced deliberately.
            group.addTask { _ = await self.loadReadings(resetSelectionIfMissing: false) }
            group.addTask { await self.loadFilters() }
        }
    }

    // ── List / search ─────────────────────────────────────────────────────

    /// Entry point for search-field edits. Debounces rapid typing so the core
    /// runs a single search once input settles (~150ms) instead of one pass per
    /// keystroke; each edit cancels the previous pending reload. Filter and
    /// refresh reloads call `loadReadings` directly and stay immediate.
    func searchDidChange() {
        searchTask?.cancel()
        invalidatePendingReadingLoads()
        hasAvailableVisualSearchSuggestion = false
        let input = activeSearchInput
        let scope = activeScope
        let imageTypeFilter = effectiveImageTypeFilter
        searchTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(150))
            guard let self, !Task.isCancelled else { return }
            // This reload fires ~150ms after the last keystroke, while the search
            // field is still focused. Preserve an unavailable focused card for
            // this reload so the state update cannot disturb the field editor and
            // let a global shortcut fire instead of editing the search term.
            let result = await loadReadings(
                resetSelectionIfMissing: !isEditingText,
                preferImmediateTextResults: true
            )
            guard result == .published,
                  !Task.isCancelled,
                  input == activeSearchInput,
                  scope == activeScope,
                  imageTypeFilter == effectiveImageTypeFilter,
                  let core
            else { return }
            hasAvailableVisualSearchSuggestion = await visualSuggestionExists(
                for: input,
                scope: scope,
                imageTypeFilter: imageTypeFilter,
                core: core
            )
        }
    }

    /// Every reading in one immutable scope/search snapshot. LazyLayoutKit
    /// virtualizes card views, so the app never waits for a trailing page.
    private func fetchReadings(
        _ core: any CoreBridging,
        context: ReadingSnapshotContext,
        candidates: ReadingSnapshotDelivery.Candidates
    ) async throws -> [ReadingRow] {
        let query = ReadingQuery.boardSnapshot(
            scope: context.scope,
            search: context.search.text,
            tagTerms: context.search.criteria.tagTerms,
            visualTerms: context.search.criteria.visualTerms,
            colorTerms: context.search.criteria.colorTerms,
            itemTypeTerms: context.search.criteria.itemTypeTerms,
            imageTypeFilter: context.imageTypeFilter,
            semanticCandidateIDs: candidates.text,
            visualSemanticCandidateIDs: candidates.visual
        )
        return try await core.listReadings(query)
    }

    private func makeSnapshotContext() -> ReadingSnapshotContext {
        readingLoadGeneration &+= 1
        return ReadingSnapshotContext(
            generation: readingLoadGeneration,
            scope: activeScope,
            search: activeSearchInput,
            imageTypeFilter: effectiveImageTypeFilter
        )
    }

    private func loadSearchCandidateIDs(
        for search: BoardSearchInput
    ) async throws -> ReadingSnapshotDelivery.Candidates {
        guard let visualSearchCoordinator else { return .empty }
        let text: [String] = if let query = search.text {
            try await visualSearchCoordinator.candidates(
                for: query,
                limit: semanticCandidateLimit
            )
        } else {
            []
        }

        return ReadingSnapshotDelivery.Candidates(
            text: text,
            visual: try await loadVisualCandidateIDs(for: search.criteria.visualTerms)
        )
    }

    private func loadVisualCandidateIDs(for terms: [String]) async throws -> [String] {
        guard let visualSearchCoordinator else { return [] }
        var candidateSets: [[String]] = []
        for term in terms {
            try await candidateSets.append(visualSearchCoordinator.candidates(
                for: term,
                limit: semanticCandidateLimit
            ))
        }
        return VisualSemanticCandidateIntersection.ranked(candidateSets)
    }

    private func visualSuggestionExists(
        for input: BoardSearchInput,
        scope: LibraryScope,
        imageTypeFilter: ImageTypeFilter,
        core: any CoreBridging
    ) async -> Bool {
        guard let draft = input.text else { return false }
        let token = BoardSearchToken(kind: .visual, value: draft)
        guard !input.criteria.semanticIdentity.contains(token.id) else { return false }

        let visualTerms = input.criteria.visualTerms + [token.value]
        let visualCandidates = (try? await loadVisualCandidateIDs(for: visualTerms)) ?? []
        var query = ReadingQuery.boardSnapshot(
            scope: scope,
            search: nil,
            tagTerms: input.criteria.tagTerms,
            visualTerms: visualTerms,
            colorTerms: input.criteria.colorTerms,
            itemTypeTerms: input.criteria.itemTypeTerms,
            imageTypeFilter: imageTypeFilter,
            semanticCandidateIDs: [],
            visualSemanticCandidateIDs: visualCandidates
        )
        query.limit = 1
        return (try? await core.listReadings(query).isEmpty == false) ?? false
    }

    private func makeSemanticCandidateLoader(
        for context: ReadingSnapshotContext,
        includeSemanticSearch: Bool
    ) -> (@MainActor () async throws -> ReadingSnapshotDelivery.Candidates)? {
        guard includeSemanticSearch,
              context.search.text != nil || context.search.criteria.hasVisualTerms,
              visualSearchCoordinator != nil else { return nil }
        return { try await self.loadSearchCandidateIDs(for: context.search) }
    }

    private func isCurrent(_ context: ReadingSnapshotContext) -> Bool {
        context.generation == readingLoadGeneration
            && context.scope == activeScope
            && context.search == activeSearchInput
            && context.imageTypeFilter == effectiveImageTypeFilter
            && !Task.isCancelled
    }

    private func invalidatePendingReadingLoads() {
        readingLoadGeneration &+= 1
    }

    /// `resetSelectionIfMissing` controls what happens when the focused card is
    /// absent from the freshly loaded board. Direct reloads prune it; a
    /// `refresh()` after a local mutation may preserve an open reading that is
    /// deliberately outside the current filter (see `refresh()`).
    @discardableResult
    func loadReadings(
        resetSelectionIfMissing: Bool = true,
        includeSemanticSearch: Bool = true,
        preferImmediateTextResults: Bool = false
    ) async -> ReadingLoadResult {
        guard !Task.isCancelled else { return .superseded }
        guard let core else { return .failed }
        let context = makeSnapshotContext()
        let semanticCandidates = makeSemanticCandidateLoader(
            for: context,
            includeSemanticSearch: includeSemanticSearch
        )
        do {
            let completed = try await ReadingSnapshotDelivery.load(
                textFirst: preferImmediateTextResults,
                fetch: { candidates in
                    try await self.fetchReadings(core, context: context, candidates: candidates)
                },
                semanticCandidates: semanticCandidates,
                isCurrent: { self.isCurrent(context) },
                publish: { rows, _ in self.publishReadings(rows, context: context) }
            )
            guard completed, isCurrent(context) else { return .superseded }

            // A text-only first result must not discard a selected semantic hit
            // that is still awaiting optional enrichment. Reconcile once the
            // captured query has finished, against its final published rows.
            if !boardSelection.selectedIDs.isEmpty || boardSelection.focusedID != nil {
                var selection = boardSelection
                selection.reconcile(
                    with: readings.map(\.id),
                    preserveUnavailableFocus: !resetSelectionIfMissing
                )
                if selection != boardSelection {
                    boardSelection = selection
                }
            }
            return .published
        } catch {
            if isCurrent(context) {
                self.error = error.localizedDescription
                return .failed
            }
            return .superseded
        }
    }

    private func publishReadings(_ rows: [ReadingRow], context: ReadingSnapshotContext) {
        let rows = rowsWithPendingEdits(rows, context: context)
        let boardContext = context.boardContext
        if publishedBoardContext != boardContext {
            publishedBoardContext = boardContext
        }
        // Reconciliation often confirms exactly the rows already displayed.
        // Keep their observation identity stable instead of invalidating the
        // board and detail hierarchy with an equal whole-array assignment.
        if readings != rows {
            readings = rows
        }
        // Body or same-path asset bytes can change while every row field stays
        // equal. Content invalidation remains independent of row publication.
        if libraryContentRefreshPending {
            libraryContentRefreshPending = false
            libraryContentGeneration &+= 1
        }
        TestHooks.recordStartupEvent("readings")
    }

    /// Replay pending edits over fetched rows before publication. A queued Tag
    /// removal remains outside exact-tag filters during older refreshes; the
    /// core still supplies the complete candidate set and all other predicates.
    private func rowsWithPendingEdits(
        _ rows: [ReadingRow], context: ReadingSnapshotContext
    ) -> [ReadingRow] {
        let session = librarySessionGeneration
        guard pendingTagEdits.hasEdits(librarySession: session)
            || pendingImageTypeEdits.hasEdits(librarySession: session) else { return rows }
        return rows.compactMap { row in
            let tagged = pendingTagEdits.apply(to: row, librarySession: session)
            let updated = pendingImageTypeEdits.apply(to: tagged, librarySession: session)
            return rowMatchesOptimisticTagFilter(
                updated, scope: context.scope, tagTerms: context.search.criteria.tagTerms,
                imageTypeFilter: context.imageTypeFilter
            ) ? updated : nil
        }
    }

    // ── Filter metadata ───────────────────────────────────────────────────

    func loadFilters() async {
        guard let core else { return }
        let session = librarySessionGeneration
        // The compatible FFI count payload still bundles legacy view/rating
        // counts. Only its global tag vocabulary is presentation state now.
        // Search and board facets must not rebuild/re-publish 13k tag values.
        guard let counts = try? await core.filterCounts(
            kind: nil, scope: .all, tag: nil, query: nil
        ) else { return }
        guard session == librarySessionGeneration else { return }
        let tags = counts.tags.map { TagCount($0) }
        if filters.tags != tags {
            filters = LibraryFilters(tags: tags)
        }
    }

    /// Reload the board after its scope changes. The global tag vocabulary only
    /// changes when library files change.
    func reloadForFilterChange() async {
        _ = await loadReadings()
    }

    // ── Filter selection ──────────────────────────────────────────────────

    /// Switch to one exact board scope.
    func selectScope(_ scope: LibraryScope) {
        guard activeScope != scope else { return }
        activeScope = scope
        invalidatePendingReadingLoads()
        Task { await reloadForFilterChange() }
    }

    func selectImageTypeFilter(_ filter: ImageTypeFilter) {
        guard activeImageTypeFilter != filter else { return }
        activeImageTypeFilter = filter
        invalidatePendingReadingLoads()
        Task { await reloadForFilterChange() }
    }
}
