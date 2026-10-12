// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

extension AppState {
    func selectReading(id: String, extending: Bool, toggling: Bool = false) {
        boardSelection.select(
            id,
            extending: extending,
            toggling: toggling,
            in: readings.map(\.id)
        )
    }

    /// A context action on an already-selected card applies to the complete
    /// board selection. Acting on any other card remains scoped to that card.
    func requestDelete(_ row: ReadingRow) {
        guard !isDeleting else { return }
        if selectedIDs.contains(row.id), !selectedRows.isEmpty {
            pendingDelete = selectedRows
        } else {
            pendingDelete = [row]
        }
    }

    func requestDeleteSelection() {
        guard !isDeleting, !selectedRows.isEmpty else { return }
        pendingDelete = selectedRows
    }
}
