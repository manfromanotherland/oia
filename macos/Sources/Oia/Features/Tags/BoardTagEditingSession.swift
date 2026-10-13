// SPDX-License-Identifier: GPL-3.0-or-later

import Observation

/// Owns the serial persistence queue independently of the dialog's lifetime, so
/// closing can dismiss immediately without abandoning already-requested edits.
@MainActor
@Observable
final class BoardTagEditingSession {
    var state: BoardTagEditorState
    @ObservationIgnored private var completionTask: Task<Void, Never>?
    @ObservationIgnored private var persistenceTasks: [UInt64: Task<Int, Never>] = [:]

    init(rows: [ReadingRow], librarySession: UInt64) {
        state = BoardTagEditorState(rows: rows, librarySession: librarySession)
    }

    func addTag(_ tag: String, using appState: AppState) {
        guard state.librarySession == appState.librarySessionGeneration,
              let edit = state.addTag(tag) else { return }
        enqueue(edit, using: appState)
    }

    func removeTag(_ tag: String, using appState: AppState) {
        guard state.librarySession == appState.librarySessionGeneration,
              let edit = state.removeTag(tag) else { return }
        enqueue(edit, using: appState)
    }

    func updateRows(_ rows: [ReadingRow], librarySession: UInt64) {
        guard state.librarySession == librarySession else { return }
        state.updateRows(rows)
    }

    private func enqueue(_ edit: BoardTagEditorState.Edit, using appState: AppState) {
        // Reserve chronology at the user action, including edits from another
        // dialog or the Inspector while this sheet's completions are pending.
        persistenceTasks[edit.id] = appState.enqueueTagEdit(
            edit.tag, ids: state.targetIDs, applies: edit.applies
        )
        guard completionTask == nil else { return }
        completionTask = Task { [self] in
            defer {
                completionTask = nil
            }
            while let edit = state.nextEdit {
                guard state.librarySession == appState.librarySessionGeneration else { return }
                let ids = state.targetIDs
                guard let persistence = persistenceTasks[edit.id] else { return }
                _ = await persistence.value

                var rows: [ReadingRow] = []
                for id in ids {
                    guard state.librarySession == appState.librarySessionGeneration else { return }
                    if let row = await appState.reloadRow(id: id) {
                        rows.append(row)
                    }
                }
                guard state.librarySession == appState.librarySessionGeneration else { return }
                state.complete(edit, rows: rows)
                persistenceTasks.removeValue(forKey: edit.id)
            }
        }
    }
}
