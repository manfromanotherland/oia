// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Transient selection on the card board. `focusedID` is the keyboard cursor;
/// `selectedIDs` is the range that board actions operate on.
struct BoardSelection<ID: Hashable & Sendable>: Equatable, Sendable {
    private(set) var focusedID: ID?
    private(set) var selectedIDs: Set<ID> = []
    private var anchorID: ID?

    var isEmpty: Bool {
        selectedIDs.isEmpty
    }

    mutating func select(_ id: ID, extending: Bool, toggling: Bool = false, in orderedIDs: [ID]) {
        if toggling, !extending {
            toggle(id, in: orderedIDs)
            return
        }
        guard extending else {
            selectOnly(id)
            return
        }

        let anchor = anchorID ?? focusedID ?? id
        guard let anchorIndex = orderedIDs.firstIndex(of: anchor),
              let focusedIndex = orderedIDs.firstIndex(of: id)
        else {
            selectOnly(id)
            return
        }

        let bounds = min(anchorIndex, focusedIndex) ... max(anchorIndex, focusedIndex)
        if toggling {
            selectedIDs.formUnion(orderedIDs[bounds])
        } else {
            selectedIDs = Set(orderedIDs[bounds])
        }
        focusedID = id
        anchorID = anchor
    }

    mutating func clear() {
        focusedID = nil
        selectedIDs = []
        anchorID = nil
    }

    mutating func selectAll(in orderedIDs: [ID]) {
        selectedIDs = Set(orderedIDs)
        if focusedID.map(selectedIDs.contains) != true {
            focusedID = orderedIDs.first
        }
        anchorID = focusedID
    }

    /// Drops cards that no longer belong to the current board snapshot. A
    /// refresh may deliberately preserve an unavailable focused reading while
    /// Gallery advances past an externally removed file.
    mutating func reconcile(with orderedIDs: [ID], preserveUnavailableFocus: Bool) {
        let availableIDs = Set(orderedIDs)
        selectedIDs.formIntersection(availableIDs)

        if let focusedID, availableIDs.contains(focusedID) {
            selectedIDs.insert(focusedID)
        } else if let visibleFocus = orderedIDs.first(where: selectedIDs.contains) {
            focusedID = visibleFocus
        } else if !preserveUnavailableFocus {
            focusedID = nil
        }

        if let anchorID, !availableIDs.contains(anchorID) {
            self.anchorID = focusedID.flatMap { availableIDs.contains($0) ? $0 : nil }
                ?? orderedIDs.first(where: selectedIDs.contains)
        }
    }

    /// Removes successfully deleted cards and puts the keyboard cursor on the
    /// next surviving card, falling back to the previous one at the end.
    mutating func remove(_ removedIDs: Set<ID>, from orderedIDs: [ID]) {
        let removedFocusedIndex = focusedID.flatMap(orderedIDs.firstIndex(of:))
        let focusedWasRemoved = focusedID.map(removedIDs.contains) ?? false
        selectedIDs.subtract(removedIDs)

        guard focusedWasRemoved else {
            if let anchorID, removedIDs.contains(anchorID) {
                self.anchorID = focusedID
            }
            return
        }

        let fallback = removedFocusedIndex.flatMap { index in
            let selectedFallback = orderedIDs.dropFirst(index + 1).first(where: selectedIDs.contains)
                ?? orderedIDs[..<index].reversed().first(where: selectedIDs.contains)
            return selectedFallback
                ?? orderedIDs.dropFirst(index + 1).first { !removedIDs.contains($0) }
                ?? orderedIDs[..<index].reversed().first { !removedIDs.contains($0) }
        }
        focusedID = fallback
        anchorID = fallback
        if let fallback {
            selectedIDs.insert(fallback)
        }
    }

    private mutating func selectOnly(_ id: ID) {
        focusedID = id
        selectedIDs = [id]
        anchorID = id
    }

    private mutating func toggle(_ id: ID, in orderedIDs: [ID]) {
        if selectedIDs.remove(id) == nil {
            selectedIDs.insert(id)
            focusedID = id
        } else if focusedID == id {
            focusedID = orderedIDs.first(where: selectedIDs.contains)
        }
        anchorID = focusedID
    }
}
