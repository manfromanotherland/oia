// SPDX-License-Identifier: GPL-3.0-or-later

// Thin wrapper around the UniFFI-generated `Database` object.
// All calls are dispatched to a background actor so the UI stays responsive.

import Foundation

actor CoreBridge {
    private let database: Database
    private let libraryPath: String
    /// A scan step may hash a complete preview, so this limits folders rather
    /// than bytes or elapsed time. Keep the step small enough to check for a
    /// board scroll frequently on ordinary libraries.
    private static let reconciliationBatchSize: UInt32 = 4

    /// Opening SQLite can run schema migrations and prepare the visual cache.
    /// Keep that work off the main actor so a restored window can paint first.
    static func open(libraryPath: String, dbPath: String) async throws -> CoreBridge {
        try await Task.detached(priority: .userInitiated) {
            try CoreBridge(libraryPath: libraryPath, dbPath: dbPath)
        }.value
    }

    init(libraryPath: String, dbPath: String) throws {
        self.libraryPath = libraryPath
        database = try Database.open(dbPath: dbPath)
    }

    // ── Indexing ──────────────────────────────────────────────────────────

    func rebuild() async throws {
        try await InteractionIdleGate.shared.waitUntilIdle()
        let sessionID = try await Self.background { [database, libraryPath] in
            try database.beginRebuild(libraryPath: libraryPath)
        }
        do {
            while true {
                try await InteractionIdleGate.shared.waitUntilIdle()
                let complete = try await Self.background { [database] in
                    try database.rebuildBatch(
                        sessionId: sessionID,
                        maxReadings: Self.reconciliationBatchSize
                    )
                }
                if complete { return }
            }
        } catch {
            // A cancelled boot must release its partial in-memory scan. This
            // cleanup runs even when the caller is already cancelled. The
            // token leaves any newer session on this bridge untouched.
            await Task.detached(priority: .utility) { [database] in
                database.abortReconciliation(sessionId: sessionID)
            }.value
            throw error
        }
    }

    @discardableResult
    func sync() async throws -> UInt32 {
        try await InteractionIdleGate.shared.waitUntilIdle()
        let sessionID = try await Self.background { [database, libraryPath] in
            try database.beginSync(libraryPath: libraryPath)
        }
        do {
            while true {
                try await InteractionIdleGate.shared.waitUntilIdle()
                let changed = try await Self.background { [database] in
                    try database.syncBatch(
                        sessionId: sessionID,
                        maxReadings: Self.reconciliationBatchSize
                    )
                }
                if let changed { return changed }
            }
        } catch {
            await Task.detached(priority: .utility) { [database] in
                database.abortReconciliation(sessionId: sessionID)
            }.value
            throw error
        }
    }

    func sync(paths: [String]) async throws -> UInt32 {
        try await InteractionIdleGate.shared.waitUntilIdle()
        let changed = try await Self.background { [database, libraryPath] in
            try database.syncPathsIfPrecise(libraryPath: libraryPath, changedPaths: paths)
        }
        if let changed { return changed }
        // An ancestor/unknown FSEvents path needs a full recovery scan. Route
        // it through the same paced path instead of Rust's synchronous legacy
        // fallback, retaining the old nonempty-event refresh signal.
        let fullChanged = try await sync()
        return max(fullChanged, paths.isEmpty ? 0 : 1)
    }

    func pendingVisualAnalysis(
        analyzerVersion: String, limit: UInt32, afterReadingID: String? = nil
    ) async throws -> PendingVisualAnalysis {
        try await InteractionIdleGate.shared.waitUntilIdle()
        return try await Self.background { [database, libraryPath] in
            try PendingVisualAnalysis(database.pendingVisualAnalysisBatch(
                libraryPath: libraryPath,
                analyzerVersion: analyzerVersion,
                afterReadingId: afterReadingID,
                limit: limit
            ))
        }
    }

    @discardableResult
    func completeVisualAnalysis(
        task: VisualAnalysisWorkItem, result: VisualAnalysisCompletion
    ) async throws -> Bool {
        try await Self.background { [database, libraryPath] in
            try database.completeVisualAnalysis(
                libraryPath: libraryPath,
                task: task.ffi,
                result: result.ffi
            )
        }
    }

    func currentVisualAssets() async throws -> [VisualAssetSnapshot] {
        var assets: [VisualAssetSnapshot] = []
        var cursor: String?
        repeat {
            try await InteractionIdleGate.shared.waitUntilIdle()
            let after = cursor
            let batch = try await Self.background { [database, libraryPath] in
                try database.currentVisualAssetsBatch(libraryPath: libraryPath, afterReadingId: after, limit: 16)
            }
            try Task.checkCancellation()
            assets.append(contentsOf: batch.assets.map(VisualAssetSnapshot.init))
            cursor = batch.nextReadingId
        } while cursor != nil
        return assets
    }

    func pendingTextTagging(
        analyzerVersion: String, limit: UInt32, afterReadingID: String?
    ) async throws -> TextTaggingBatch {
        try await InteractionIdleGate.shared.waitUntilIdle()
        return try await Self.background { [database, libraryPath] in
            try TextTaggingBatch(database.pendingTextTagging(
                libraryPath: libraryPath, analyzerVersion: analyzerVersion,
                limit: limit, afterReadingId: afterReadingID
            ))
        }
    }

    @discardableResult
    func completeTextTagging(task: TextTaggingWorkItem, tags: [String]) async throws -> Bool {
        try await Self.background { [database, libraryPath] in
            try database.completeTextTagging(libraryPath: libraryPath, task: task.ffi, tags: tags)
        }
    }

    /// Filesystem work can wait on external storage. Release the interactive
    /// actor while it runs; Rust keeps only short DB snapshots/commits locked.
    private nonisolated static func background<Value: Sendable>(
        _ operation: @escaping @Sendable () throws -> Value
    ) async throws -> Value {
        try Task.checkCancellation()
        let task = Task.detached(priority: .utility) {
            try Task.checkCancellation()
            return try operation()
        }
        return try await withTaskCancellationHandler {
            try await task.value
        } onCancel: {
            task.cancel()
        }
    }

    // ── Query ─────────────────────────────────────────────────────────────

    /// Readings for the composed scope/tag filter, the board order,
    /// and an optional full-text query. Dormant rating support remains nil at the
    /// FFI boundary for library-format compatibility.
    func listReadings(_ query: ReadingQuery) throws -> [ReadingRow] {
        let opts = FfiListOptions(
            view: query.scope.ffiView,
            sort: query.sort.ffiSort,
            ascending: query.ascending,
            tag: query.tag,
            rating: nil,
            kind: query.kind?.ffiKind,
            since: nil, until: nil,
            query: query.search,
            tagTerms: query.tagTerms,
            visualTerms: query.visualTerms,
            colorTerms: query.colorTerms,
            itemTypeTerms: query.itemTypeTerms,
            predominantColor: nil,
            semanticCandidateIds: query.semanticCandidateIDs,
            visualSemanticCandidateIds: query.visualSemanticCandidateIDs,
            limit: query.limit, offset: query.offset
        )
        return try database.listReadings(opts: opts).map(ReadingRow.init)
    }

    func readingCount() throws -> UInt64 {
        try filterCounts(kind: nil, scope: .all, tag: nil, query: nil).views.all
    }

    /// Reuse the compatible count payload to enumerate tags for the app's filter
    /// menu. Legacy view and rating counts remain dormant at the FFI boundary.
    func filterCounts(
        kind: ReadingKind?, scope: LibraryScope, tag: String?, query: String?
    ) throws -> FfiSidebarCounts {
        let ffiScope = FfiCountScope(
            view: scope.ffiView,
            tag: tag,
            rating: nil,
            kind: kind?.ffiKind,
            query: query,
            tagTerms: [],
            visualTerms: [],
            predominantColor: nil,
            semanticCandidateIds: [],
            visualSemanticCandidateIds: []
        )
        return try database.sidebarCounts(scope: ffiScope)
    }

    func getReadingRow(id: String) throws -> FfiReadingRow? {
        try database.getReadingRow(id: id)
    }

    func getBody(id: String) throws -> String? {
        try database.getBody(id: id)
    }

    func getReadingInspector(id: String) async throws -> ReadingInspector? {
        try await Self.background { [database, libraryPath] in
            try database.getReadingInspector(libraryPath: libraryPath, id: id)
        }
    }

    // ── Imports ───────────────────────────────────────────────────────────

    /// Inbox ingestion only writes library files, not the database. Release
    /// this actor while the batch copies and verifies media so existing reader
    /// and search queries remain responsive. The caller serializes Inbox passes
    /// and reconciles the index once after the batch finishes.
    func processInbox() async throws -> FfiInboxReport {
        try await Task.detached(priority: .utility) { [database, libraryPath] in
            let libraryURL = URL(fileURLWithPath: libraryPath, isDirectory: true)
            try LibrarySetup.scaffold(at: libraryURL)
            let deferredNames = try InboxFileAvailability.deferredNames(
                in: libraryURL.appendingPathComponent("inbox", isDirectory: true)
            )
            let python = FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/Application Support/Oia/Instagram/bin/python3")
            return try database.processInboxWithInstagram(
                libraryPath: libraryPath, deferredNames: deferredNames,
                pythonPath: python.path,
                scriptPath: Bundle.main.url(forResource: "instagram-download", withExtension: "py")?.path ?? ""
            )
        }.value
    }

    /// Recognised public sources may perform bounded network retrieval before
    /// committing. Release this actor while the Rust URL-save facade works so
    /// existing reads remain responsive.
    func importLink(url: String) async throws -> FfiImportResult {
        try await Task.detached(priority: .utility) { [database, libraryPath] in
            try database.importLink(libraryPath: libraryPath, url: url)
        }.value
    }

    func importText(text: String, title: String?) async throws -> FfiImportResult {
        try await Self.background { [database, libraryPath] in
            try database.importText(libraryPath: libraryPath, text: text, title: title)
        }
    }

    func importImage(data: Data, contentType: String, title: String) async throws -> FfiImportResult {
        try await Self.background { [database, libraryPath] in
            try database.importImage(
                libraryPath: libraryPath,
                bytes: data,
                contentType: contentType,
                title: title
            )
        }
    }

    /// The staged movie stays file-backed across the FFI boundary so large
    /// videos are never copied into a Swift or UniFFI byte buffer.
    func importVideoFile(
        filePath: String, contentType: String, title: String
    ) async throws -> FfiImportResult {
        try await Self.background { [database, libraryPath] in
            try database.importVideoFile(
                libraryPath: libraryPath,
                filePath: filePath,
                contentType: contentType,
                title: title
            )
        }
    }

    // ── Tags ──────────────────────────────────────────────────────────────

    func addTag(id: String, tag: String) async throws {
        try await Self.background { [database, libraryPath] in
            try database.addTag(libraryPath: libraryPath, id: id, tag: tag)
        }
    }

    func removeTag(id: String, tag: String) async throws {
        try await Self.background { [database, libraryPath] in
            try database.removeTag(libraryPath: libraryPath, id: id, tag: tag)
        }
    }

    // ── Deletion ──────────────────────────────────────────────────────────

    /// Permanently delete a reading (file, assets, and index row).
    func deleteReading(id: String) async throws {
        try await Self.background { [database, libraryPath] in
            try database.deleteReading(libraryPath: libraryPath, id: id)
        }
    }

    // ── Highlights ────────────────────────────────────────────────────────

    func listHighlights(readingId: String) throws -> [FfiHighlight] {
        try database.listHighlights(libraryPath: libraryPath, readingId: readingId)
    }

    @discardableResult
    func addHighlight(readingId: String, text: String) throws -> FfiHighlight {
        try database.addHighlight(libraryPath: libraryPath, readingId: readingId, text: text)
    }

    /// Toggle a highlight by its text. Returns `true` if highlighted after the
    /// call, `false` if it was cleared.
    @discardableResult
    func toggleHighlight(readingId: String, text: String) throws -> Bool {
        try database.toggleHighlight(libraryPath: libraryPath, readingId: readingId, text: text)
    }

    func deleteHighlight(readingId: String, highlightId: String) throws {
        try database.deleteHighlight(libraryPath: libraryPath, readingId: readingId, highlightId: highlightId)
    }
}
