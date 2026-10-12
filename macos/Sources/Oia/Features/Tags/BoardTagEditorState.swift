// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Presentation for one frozen board-tagging session. Pending edits are shown
/// immediately and replayed over refreshed rows until their file write settles.
/// The refreshed rows, rather than a manual rollback, then supply the truth.
struct BoardTagEditorState {
    struct Tag: Identifiable, Equatable {
        let name: String
        let isMachine: Bool
        var id: Data {
            ExactTagIdentity.bytes(name)
        }
    }

    struct Edit: Identifiable, Equatable {
        let id: UInt64
        let tag: String
        let applies: Bool
    }

    let targetIDs: [String]
    let librarySession: UInt64

    private let targetIDSet: Set<String>
    private var rowsByID: [String: ReadingRow]
    private var pendingEdits: [Edit] = []
    private var addedNames: [String] = []
    private var editGeneration: UInt64 = 0

    init(rows: [ReadingRow], librarySession: UInt64) {
        var seen: Set<String> = []
        targetIDs = rows.map(\.id).filter { seen.insert($0).inserted }
        targetIDSet = seen
        rowsByID = rows.reduce(into: [:]) { result, row in
            if result[row.id] == nil {
                result[row.id] = row
            }
        }
        self.librarySession = librarySession
    }

    /// Persist in order. Another edit can be queued while this one is awaiting
    /// the core, without an older completion replacing the newer presentation.
    var nextEdit: Edit? {
        pendingEdits.first
    }

    /// A single card exposes every effective Tag, including machine ownership.
    /// Bulk editing exposes only names added during this tagging session.
    var tags: [Tag] {
        let rows = presentedRows
        if targetIDs.count == 1, let row = rows.first {
            return row.tags.map { name in
                Tag(
                    name: name,
                    isMachine: row.machineTags.contains { ExactTagIdentity.matches($0, name) }
                )
            }
        }

        return addedNames.compactMap { addedName in
            let matchingRows = rows.filter { row in
                row.tags.contains { ExactTagIdentity.matches($0, addedName) }
            }
            guard let first = matchingRows.first,
                  let name = first.tags.first(where: { ExactTagIdentity.matches($0, addedName) })
            else { return nil }
            return Tag(
                name: name,
                isMachine: matchingRows.allSatisfy { row in
                    row.machineTags.contains { ExactTagIdentity.matches($0, name) }
                }
            )
        }
    }

    /// Suggestions omit names already owned by the user on every target. A
    /// matching machine Tag remains addable, so adding it transfers ownership.
    var commonUserTags: [String] {
        let rows = presentedRows
        guard rows.count == targetIDs.count, let first = rows.first else { return [] }
        return first.tags.filter { name in
            rows.allSatisfy { row in
                row.tags.contains { ExactTagIdentity.matches($0, name) }
                    && !row.machineTags.contains { ExactTagIdentity.matches($0, name) }
            }
        }
    }

    @discardableResult
    mutating func addTag(_ name: String) -> Edit? {
        let tag = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !targetIDs.isEmpty, !tag.isEmpty, TagRules.isWithinLength(tag) else { return nil }
        if !addedNames.contains(where: { ExactTagIdentity.matches($0, tag) }) {
            addedNames.append(tag)
        }
        return enqueue(tag: tag, applies: true)
    }

    @discardableResult
    mutating func removeTag(_ name: String) -> Edit? {
        let tag = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !targetIDs.isEmpty, !tag.isEmpty else { return nil }
        return enqueue(tag: tag, applies: false)
    }

    /// A board filter may hide a target after removal. Absence from a visible
    /// snapshot never discards its retained editor snapshot or changes targets.
    mutating func updateRows(_ rows: [ReadingRow]) {
        for row in rows where targetIDSet.contains(row.id) {
            rowsByID[row.id] = row
        }
    }

    /// Call after refreshing the exact target rows, including any no longer on
    /// the board. Failed writes then naturally reappear as their stored state.
    /// Missing exact-row results mean unavailable targets and discard old Tags.
    /// Later queued edits remain optimistic over the authoritative snapshot.
    mutating func complete(_ edit: Edit, rows: [ReadingRow]) {
        guard pendingEdits.first?.id == edit.id else { return }
        rowsByID = rows.reduce(into: [:]) { result, row in
            if targetIDSet.contains(row.id) {
                result[row.id] = row
            }
        }
        pendingEdits.removeFirst()
    }

    private var presentedRows: [ReadingRow] {
        targetIDs.compactMap { id in
            guard var row = rowsByID[id] else { return nil }
            for edit in pendingEdits {
                row.applyTagEdit(edit.tag, applies: edit.applies)
            }
            return row
        }
    }

    private mutating func enqueue(tag: String, applies: Bool) -> Edit {
        editGeneration &+= 1
        let edit = Edit(id: editGeneration, tag: tag, applies: applies)
        pendingEdits.append(edit)
        return edit
    }
}
