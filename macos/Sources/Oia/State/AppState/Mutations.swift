// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Captured before queueing so later UI navigation cannot retarget a Tag edit.
private struct TagEditRequest {
    let tag: String
    let ids: [String]
    let applies: Bool
    let core: any CoreBridging
    let librarySession: UInt64
    let reservation: UInt64
}

// ── Mutations ────────────────────────────────────────────────────────────────
// Tag edits land optimistically, then the core write + `refresh()` reconcile.
// Deletion follows the same pattern; the refresh restores any failed rows.

extension AppState {
    /// Mirror only scope and exact Tag predicates for optimistic removals.
    /// Free-text, visual predicates, and missing-row inclusion stay in the core.
    func rowMatchesOptimisticTagFilter(
        _ row: ReadingRow, scope: LibraryScope, tagTerms: [String]
    ) -> Bool {
        ComposedFilter.matches(row, scope: scope) && tagTerms.allSatisfy { requiredTag in
            row.tags.contains { ExactTagIdentity.matches($0, requiredTag) }
        }
    }

    /// If optimistic edits pushed rows out of the current filter, slide them
    /// out and advance selection to an adjacent row in the *same* render tick —
    /// one motion, not
    /// an in-place icon flip followed a beat later by the row jumping away. A
    /// no-op when every row still matches. Free-text and visual predicates are
    /// not mirrored in Swift, but a failed scope or exact-tag predicate is
    /// sufficient to know the complete AND query can no longer match.
    private func advancePastFilteredRows(ids: Set<String>) {
        let requiredTags = activeSearchInput.criteria.tagTerms
        let removedIDs = Set(readings.compactMap { row -> String? in
            guard ids.contains(row.id) else { return nil }
            return rowMatchesOptimisticTagFilter(row, scope: activeScope, tagTerms: requiredTags)
                ? nil : row.id
        })
        guard !removedIDs.isEmpty else { return }
        withAnimation {
            boardSelection.remove(removedIDs, from: readings.map(\.id))
            readings.removeAll { removedIDs.contains($0.id) }
        }
    }

    /// Optimistically remove readings in one motion, then permanently delete
    /// their files and reconcile once. Callers should confirm the complete set.
    func delete(_ rows: [ReadingRow]) async {
        guard let core, !rows.isEmpty, !isDeleting else { return }
        isDeleting = true
        beginLibraryWrite()
        defer {
            endLibraryWrite()
            isDeleting = false
        }
        let boardOrder = readings.map(\.id)
        let requestedIDs = Set(rows.map(\.id))
        var deletedIDs: Set<String> = []
        var failures: [(ReadingRow, Error)] = []

        withAnimation {
            boardSelection.remove(requestedIDs, from: boardOrder)
            readings.removeAll { requestedIDs.contains($0.id) }
        }

        for row in rows {
            do {
                try await core.deleteReading(id: row.id)
                deletedIDs.insert(row.id)
            } catch {
                failures.append((row, error))
            }
        }

        await refresh()
        scheduleVisualSearchReconciliation()
        reportDeleteFailures(failures, deletedCount: deletedIDs.count, requestedCount: rows.count)
    }

    private func reportDeleteFailures(
        _ failures: [(ReadingRow, Error)],
        deletedCount: Int,
        requestedCount: Int
    ) {
        if let firstFailure = failures.first {
            if deletedCount == 0, failures.count == 1 {
                error = firstFailure.1.localizedDescription
            } else if failures.count == 1 {
                error = "Deleted \(deletedCount) of \(requestedCount) selected items. "
                    + "Unable to delete “\(firstFailure.0.displayTitle)”; try again."
            } else {
                error = "Deleted \(deletedCount) of \(requestedCount) selected items. "
                    + "Unable to delete \(failures.count), including "
                    + "“\(firstFailure.0.displayTitle)”; try again."
            }
        }
    }

    func addTag(id: String, tag: String) async {
        await addTag(tag, to: [id])
    }

    /// Add one user Tag to every requested card without replacing its existing
    /// Tags. Patch the complete visible selection before the first file write,
    /// then reconcile once so failed writes return to file-backed truth.
    @discardableResult
    func addTag(_ tag: String, to ids: [String]) async -> Int {
        await enqueueTagEdit(tag, ids: ids, applies: true).value
    }

    func removeTag(id: String, tag: String) async {
        await removeTag(tag, from: [id])
    }

    /// Remove one Tag from the frozen targets, including machine Tags whose
    /// exclusion is persisted by the core. The whole visible batch changes
    /// before any write; a single refresh reconciles successes and failures.
    @discardableResult
    func removeTag(_ tag: String, from ids: [String]) async -> Int {
        await enqueueTagEdit(tag, ids: ids, applies: false).value
    }

    /// Reserve a Tag edit in the shared persistence order at the UI action.
    /// Apply its presentation and freeze the core, library session, and target
    /// IDs before returning; persistence survives the submitting view's lifetime.
    @discardableResult
    func enqueueTagEdit(_ tag: String, ids: [String], applies: Bool) -> Task<Int, Never> {
        let tag = tag.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let core, !ids.isEmpty, !tag.isEmpty,
              !applies || TagRules.isWithinLength(tag) else { return Task { 0 } }
        var seenIDs: Set<String> = []
        let requestedIDs = ids.filter { seenIDs.insert($0).inserted }
        let request = TagEditRequest(
            tag: tag,
            ids: requestedIDs,
            applies: applies,
            core: core,
            librarySession: librarySessionGeneration,
            reservation: pendingTagEdits.reserve(
                tag: tag, ids: seenIDs, applies: applies, librarySession: librarySessionGeneration
            )
        )
        beginLibraryWrite()
        applyQueuedTagEdit(request, ids: seenIDs)
        return tagPersistenceQueue.enqueue { [self] in
            defer {
                pendingTagEdits.complete(request.reservation)
                endLibraryWrite()
            }
            return await persistTagEdit(request)
        }
    }

    private func applyQueuedTagEdit(_ request: TagEditRequest, ids: Set<String>) {
        // Adding a matching machine name transfers ownership in presentation;
        // the core persists ownership or removal exclusions in the local file.
        for index in readings.indices where ids.contains(readings[index].id) {
            readings[index].applyTagEdit(request.tag, applies: request.applies)
        }
        if !request.applies {
            advancePastFilteredRows(ids: ids)
        }
    }

    private func persistTagEdit(_ request: TagEditRequest) async -> Int {
        guard request.librarySession == librarySessionGeneration else { return 0 }
        var successfulCount = 0
        var failures: [Error] = []
        for id in request.ids {
            guard request.librarySession == librarySessionGeneration else { return successfulCount }
            do {
                if request.applies {
                    try await request.core.addTag(id: id, tag: request.tag)
                } else {
                    try await request.core.removeTag(id: id, tag: request.tag)
                }
                successfulCount += 1
            } catch {
                failures.append(error)
            }
        }
        guard request.librarySession == librarySessionGeneration else { return successfulCount }
        pendingTagEdits.complete(request.reservation)
        await refresh()
        guard request.librarySession == librarySessionGeneration else { return successfulCount }
        reportTagFailures(failures, request: request, successfulCount: successfulCount)
        return successfulCount
    }

    private func reportTagFailures(
        _ failures: [Error], request: TagEditRequest, successfulCount: Int
    ) {
        guard let firstFailure = failures.first else { return }
        if request.ids.count == 1 {
            error = firstFailure.localizedDescription
        } else if request.applies {
            error = "Added “\(request.tag)” to \(successfulCount) of \(request.ids.count) cards. "
                + "Unable to tag \(failures.count); try again."
        } else {
            error = "Removed “\(request.tag)” from \(successfulCount) of \(request.ids.count) cards. "
                + "Unable to remove it from \(failures.count); try again."
        }
    }

    func getBody(id: String) async -> String? {
        guard let core else { return nil }
        return try? await core.getBody(id: id)
    }

    /// Re-fetch a single reading row from the index (e.g. after a tag edit) so
    /// detail views can refresh their local copy without a full list reload.
    func reloadRow(id: String) async -> ReadingRow? {
        guard let core else { return nil }
        let session = librarySessionGeneration
        while session == librarySessionGeneration {
            let revision = pendingTagEdits.revision
            let fetched = try? await core.getReadingRow(id: id)
            guard session == librarySessionGeneration else { return nil }
            // A newer edit may settle while this raw snapshot crosses actors.
            // Re-read instead of replacing it with a now-obsolete snapshot or
            // treating a valid target as unavailable after optimistic replay.
            guard revision == pendingTagEdits.revision else { continue }
            guard let fetched else { return nil }
            return pendingTagEdits.apply(to: ReadingRow(fetched), librarySession: session)
        }
        return nil
    }
}
