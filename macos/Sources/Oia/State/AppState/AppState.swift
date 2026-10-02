// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Observation
import SwiftUI

// The behavior lives in sibling extension files in this folder, one per
// concern: Library.swift (onboarding, boot, sync), Readings.swift (list
// loading, search, filter reloads), Mutations.swift (row edits with
// optimistic UI) and Highlights.swift. This file holds the stored state,
// `init`, and lifecycle plumbing.
@MainActor
@Observable
final class AppState {
    /// Keys the current board scope and search box persist under, plus retired
    /// kind/tag keys that are cleared during migration.
    private enum FilterDefaultsKey {
        static let kind = "selectedKind"
        static let scope = "activeScope"
        static let tag = "selectedTag"
        static let search = "searchQuery"
        static let searchTokens = "searchTokens"
        static let legacyView = "activeView"
        static let legacyRating = "selectedRating"
    }

    /// ── Navigation state ──────────────────────────────────────────────────
    var libraryURL: URL?

    /// True from launch until the first boot from a persisted bookmark settles.
    /// The restored URL is published immediately so the normal window shell and
    /// toolbar can render while the core hydrates; this flag keeps the board
    /// itself neutral and prevents onboarding from flashing if restoration fails
    /// before that first frame.
    var isRestoringLibrary: Bool = false

    var readings: [ReadingRow] = []
    /// The scope and normalized query that produced `readings`. This changes
    /// only when a result set for a new board context is actually published, so
    /// same-query enrichment and ordinary refreshes preserve the scroll position.
    var publishedBoardContext = BoardQueryContext(
        scope: .all,
        search: BoardSearchInput(text: "", tokens: [])
    )
    var boardSelection = BoardSelection<String>()

    /// The focused card remains the compatibility-facing selection for Gallery
    /// and the existing single-item reader actions.
    var selectedId: String? {
        get { boardSelection.focusedID }
        set {
            if let newValue {
                boardSelection.select(
                    newValue,
                    extending: false,
                    in: readings.map(\.id)
                )
            } else {
                boardSelection.clear()
            }
        }
    }

    var selectedIDs: Set<String> {
        boardSelection.selectedIDs
    }

    var selectedRows: [ReadingRow] {
        readings.filter { selectedIDs.contains($0.id) }
    }

    /// The reading-list search text, persisted across launches so a search the
    /// user left active is restored on reopen. `init` seeds it from the store.
    var searchQuery: String = "" {
        didSet {
            AppDefaults.store.set(searchQuery, forKey: FilterDefaultsKey.search)
        }
    }

    /// Completed native search-field tokens. Their value and internal scope are
    /// persisted with the draft query so reopening the app restores one coherent
    /// search instead of silently widening it.
    var searchTokens: [BoardSearchToken] = [] {
        didSet {
            guard let data = try? JSONEncoder().encode(searchTokens) else { return }
            AppDefaults.store.set(data, forKey: FilterDefaultsKey.searchTokens)
        }
    }

    /// True only after the current draft has produced at least one result as
    /// an exact visual token in the current board scope.
    var hasAvailableVisualSearchSuggestion = false

    var activeSearchInput: BoardSearchInput {
        BoardSearchInput(text: searchQuery, tokens: searchTokens)
    }

    func clearSearch() {
        guard !searchQuery.isEmpty || !searchTokens.isEmpty else { return }
        hasAvailableVisualSearchSuggestion = false
        searchQuery = ""
        searchTokens = []
    }

    /// Pending debounced search reload. Each keystroke cancels the previous one
    /// so the core is queried once typing settles, not per character. Plumbing
    /// only — not part of the observable UI state.
    @ObservationIgnored var searchTask: Task<Void, Never>?
    @ObservationIgnored var saveNoticeTask: Task<Void, Never>?
    @ObservationIgnored var visualSearchTask: Task<Void, Never>?
    @ObservationIgnored var visualSearchRerunPending = false
    @ObservationIgnored var readingLoadGeneration: UInt64 = 0

    /// The active library scope. Always exactly one; `.all` is the unfiltered
    /// base. Persisted under a new key so legacy unread/read/archive selections
    /// cannot become invisible filters after those features disappear.
    var activeScope: LibraryScope = .all {
        didSet {
            AppDefaults.store.set(activeScope.rawValue, forKey: FilterDefaultsKey.scope)
        }
    }

    /// Readings awaiting delete confirmation, in their current board order.
    var pendingDelete: [ReadingRow]?

    /// Drives the highlights inspector for the open reading. Opened from the
    /// Article menu and its ⌘⇧H shortcut.
    var showHighlights: Bool = false

    /// Drives the popover telling the user to select some text first, raised when
    /// the toolbar's Highlight button is pressed with nothing selected.
    var showHighlightHint: Bool = false

    /// Drives the keyboard-shortcuts cheat sheet (the ⌘/ command).
    var showShortcuts: Bool = false

    /// Highlights for the currently open reading. Drives both the reader's
    /// in-text tinting and the highlights inspector.
    var highlights: [HighlightRow] = []

    // ── Filter metadata ───────────────────────────────────────────────────

    /// Global tag names used by each reading's tag editor.
    var filters = LibraryFilters()

    /// ── Status ────────────────────────────────────────────────────────────
    var isLoading: Bool = false
    /// File reconciliation can continue after a trusted cached board is visible.
    /// Kept separate from `isLoading` so it never hides usable cached readings.
    var isReconcilingLibrary: Bool = false
    var error: String?
    var isDeleting: Bool = false
    var activeLibraryWriteCount = 0
    /// Advances whenever the active core publishes newer file-backed content.
    /// Detail views include it in their load identity so an already-open cached
    /// reading revalidates after reconciliation or an incremental sync.
    var libraryContentGeneration: UInt64 = 0
    var visualAnalysisGeneration: UInt64 = 0

    var canChangeLibrary: Bool {
        !isReconcilingLibrary && !isSaving && activeLibraryWriteCount == 0
    }

    func beginLibraryWrite() {
        activeLibraryWriteCount += 1
    }

    func endLibraryWrite() {
        activeLibraryWriteCount -= 1
    }

    /// Short, non-modal acknowledgement for paste/drop saves. Errors that stop
    /// the whole operation still use `error`; duplicates and partial results are
    /// routine status and stay out of an alert.
    var saveNotice: SaveNotice?
    var isSaving: Bool = false

    var isProcessingInbox = false
    var inboxPendingCount: UInt32 = 0
    var inboxIssues: [FfiInboxIssue] = []
    var inboxError: String?

    /// True while the user is editing a text field (the toolbar search field, the
    /// tag picker, …). macOS dispatches menu/context-menu key-equivalents *before*
    /// the focused field editor, so global shortcuts can fire mid-edit instead of
    /// editing the line. Commands whose shortcuts collide with the
    /// field editor's own keys disable themselves while this holds, letting the
    /// keystroke fall through to standard text editing. Kept in sync by
    /// `editingMonitor` (see `TextEditingMonitor`).
    var isEditingText: Bool = false
    var isFocusMode: Bool = false

    // Internal rather than private so the sibling extension files in this
    // folder can reach them — Swift's `private` is file-scoped.
    let semanticCandidateLimit = 2000
    var core: (any CoreBridging)?
    @ObservationIgnored var activeCoreID: ObjectIdentifier?
    @ObservationIgnored var hasUsableCachedLibrary = false
    @ObservationIgnored var librarySessionGeneration: UInt64 = 0
    @ObservationIgnored var libraryContentRefreshPending = false
    @ObservationIgnored var watcherSyncTask: Task<Void, Never>?
    @ObservationIgnored var watcherSyncPending = false
    @ObservationIgnored var watcherChanges = FolderWatcher.Change()
    @ObservationIgnored var inboxRetryTask: Task<Void, Never>?
    @ObservationIgnored var inboxRetryAttempt = 0
    var watcher: FolderWatcher?
    let visualSearchCoordinator: VisualSearchCoordinator?
    let textTaggingCoordinator: TextTaggingCoordinator?

    private var editingMonitor: TextEditingMonitor?

    init() {
        visualSearchCoordinator = Self.makeVisualSearchCoordinator()
        textTaggingCoordinator = TestHooks.isIsolatedRun ? nil : TextTaggingCoordinator(tagger: AppleSubjectTagger())

        let defaults = AppDefaults.store

        // Removed sort controls must not leave an invisible preference behind.
        // The board now always uses its fixed newest-first / relevance ordering.
        defaults.removeObject(forKey: "sortField")
        defaults.removeObject(forKey: "sortAscending")
        defaults.removeObject(forKey: FilterDefaultsKey.kind)
        defaults.removeObject(forKey: FilterDefaultsKey.tag)

        // Restore a current scope. Retired read/archive/favorite/note values become
        // All so a removed control can never leave an invisible filter active.
        // The old rating filter is discarded.
        let persistedScope = defaults.string(forKey: FilterDefaultsKey.scope)
            .flatMap(LibraryScope.init(rawValue:))
        activeScope = persistedScope ?? .all
        defaults.set(activeScope.rawValue, forKey: FilterDefaultsKey.scope)
        defaults.removeObject(forKey: FilterDefaultsKey.legacyView)
        defaults.removeObject(forKey: FilterDefaultsKey.legacyRating)
        searchQuery = defaults.string(forKey: FilterDefaultsKey.search) ?? ""
        if let data = defaults.data(forKey: FilterDefaultsKey.searchTokens),
           let tokens = try? JSONDecoder().decode([BoardSearchToken].self, from: data)
        {
            searchTokens = BoardSearchCriteria(tokens: tokens).tokens
        }

        if TestHooks.isIsolatedRun {
            // UI-testing: never resolve the persisted bookmark (leave the dev's
            // real library untouched). Boot the pinned temp library if one was
            // given; otherwise fall through to the onboarding screen.
            if let path = TestHooks.libraryPath {
                let url = URL(fileURLWithPath: path)
                libraryURL = url
                isRestoringLibrary = true
                Task { await boot(url: url) }
            }
        } else if let url = LibraryBookmark.resolve() {
            libraryURL = url
            isRestoringLibrary = true
            Task { await boot(url: url) }
        }

        editingMonitor = TextEditingMonitor { [weak self] editing in
            self?.isEditingText = editing
        }
    }

    private static func makeVisualSearchCoordinator() -> VisualSearchCoordinator? {
        // Never donate throwaway fixture IDs to the user's real Spotlight index
        // or introduce system-model timing into UI tests.
        guard !TestHooks.isIsolatedRun else { return nil }
        return VisualSearchCoordinator(
            analyzer: AppleVisualAnalyzer(),
            analyzerVersion: AppleVisualAnalyzer.analyzerVersion,
            spotlight: SpotlightVisualIndex()
        )
    }
}
