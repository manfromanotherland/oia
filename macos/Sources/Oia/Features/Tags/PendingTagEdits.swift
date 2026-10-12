// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Replays requested presentation edits over file-backed rows until each
/// persistence reservation completes. An older refresh cannot undo a newer
/// queued action, and completion lets the stored row restore failed edits.
struct PendingTagEdits {
    private struct Edit {
        let id: UInt64
        let tag: String
        let ids: Set<String>
        let applies: Bool
        let librarySession: UInt64
    }

    private var edits: [Edit] = []
    private var generation: UInt64 = 0
    /// Exact-row reads retry if reservations changed while their read awaited,
    /// so a stale snapshot cannot escape after its newer overlay has settled.
    private(set) var revision: UInt64 = 0

    /// Reserve synchronously with the user action, before its persistence Task
    /// can suspend. Callers validate the name before reserving a real edit.
    @discardableResult
    mutating func reserve(
        tag: String,
        ids: Set<String>,
        applies: Bool,
        librarySession: UInt64
    ) -> UInt64 {
        generation &+= 1
        edits.append(Edit(
            id: generation,
            tag: tag.trimmingCharacters(in: .whitespacesAndNewlines),
            ids: ids,
            applies: applies,
            librarySession: librarySession
        ))
        revision &+= 1
        return generation
    }

    /// Remove only the settled reservation. Other requested edits stay
    /// optimistic, including later edits to the same Tag on the same reading.
    mutating func complete(_ id: UInt64) {
        guard let index = edits.firstIndex(where: { $0.id == id }) else { return }
        edits.remove(at: index)
        revision &+= 1
    }

    func apply(to row: ReadingRow, librarySession: UInt64) -> ReadingRow {
        var presented = row
        for edit in edits where edit.librarySession == librarySession && edit.ids.contains(row.id) {
            presented.applyTagEdit(edit.tag, applies: edit.applies)
        }
        return presented
    }

    func hasEdits(librarySession: UInt64) -> Bool {
        edits.contains { $0.librarySession == librarySession }
    }
}
