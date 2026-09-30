// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

// ── Mutations ────────────────────────────────────────────────────────────────
// Tag edits land optimistically, then the core write + `refresh()` reconcile.
// Deletion follows the same pattern; the refresh restores any failed rows.

extension AppState {
    /// Optimistically apply an edit so it shows on the next frame, before the
    /// core write + `refresh()` land by swapping the row in the visible list (if
    /// present). Removing a row that no longer matches the filter is the explicit
    /// job of `advancePastFilteredRow`; re-ordering is left to the follow-up
    /// `refresh()`. A failed write self-heals from the index.
    private func applyOptimistic(_ old: ReadingRow, _ new: ReadingRow) {
        if let index = readings.firstIndex(where: { $0.id == old.id }) {
            readings[index] = new
        }
    }

    /// Mirror of the core's board scope: does `row` still belong in the list the
    /// user is currently looking at?
    private func rowMatchesCurrentFilter(_ row: ReadingRow) -> Bool {
        ComposedFilter.matches(row, scope: activeScope)
    }

    /// Pair with `applyOptimistic`: if the optimistic edit
    /// pushed the row out of the current filter, slide it out and advance the
    /// selection to an adjacent row in the *same* render tick — one motion, not
    /// an in-place icon flip followed a beat later by the row jumping away. A
    /// no-op when the row still matches. Free-text and visual predicates are
    /// not mirrored in Swift, but a failed scope or exact-tag predicate is
    /// sufficient to know the complete AND query can no longer match.
    private func advancePastFilteredRow(id: String) {
        guard let index = readings.firstIndex(where: { $0.id == id }) else { return }
        let row = readings[index]
        let matchesRequiredTags = activeSearchInput.criteria.tagTerms.allSatisfy { requiredTag in
            row.tags.contains { ExactTagIdentity.matches($0, requiredTag) }
        }
        guard !rowMatchesCurrentFilter(row) || !matchesRequiredTags else { return }
        withAnimation {
            boardSelection.remove([id], from: readings.map(\.id))
            readings.remove(at: index)
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
        guard let core else { return }
        beginLibraryWrite()
        defer { endLibraryWrite() }
        // A manually added name takes ownership of a matching machine tag.
        let tag = tag.trimmingCharacters(in: .whitespaces)
        if let old = readings.first(where: { $0.id == id }) {
            var updated = old
            updated.applyTagEdit(tag, applies: true)
            applyOptimistic(old, updated)
        }
        try? await core.addTag(id: id, tag: tag)
        await refresh()
    }

    func removeTag(id: String, tag: String) async {
        guard let core else { return }
        beginLibraryWrite()
        defer { endLibraryWrite() }
        if let old = readings.first(where: { $0.id == id }),
           old.tags.contains(where: { ExactTagIdentity.matches($0, tag) })
        {
            var updated = old
            updated.applyTagEdit(tag, applies: false)
            applyOptimistic(old, updated)
            advancePastFilteredRow(id: id)
        }
        try? await core.removeTag(id: id, tag: tag)
        await refresh()
    }

    func getBody(id: String) async -> String? {
        guard let core else { return nil }
        return try? await core.getBody(id: id)
    }

    /// Re-fetch a single reading row from the index (e.g. after a tag edit) so
    /// detail views can refresh their local copy without a full list reload.
    func reloadRow(id: String) async -> ReadingRow? {
        guard let core else { return nil }
        guard let fetched = try? await core.getReadingRow(id: id) else { return nil }
        return ReadingRow(fetched)
    }
}
