// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Freezes the opening board order and indexes surviving neighbors once per
/// collection change. Gallery rendering and arrow navigation only read it.
struct GallerySnapshot<Row: Identifiable & Equatable> {
    private var order: [Row.ID]
    private var positions: [Row.ID: Int] = [:]
    private var previous: [Row.ID: Row.ID] = [:]
    private var next: [Row.ID: Row.ID] = [:]
    private(set) var rows: [Row] = []

    init(_ rows: [Row] = []) {
        order = rows.map(\.id)
        reconcile(rows)
    }

    /// A gallery opened from a board preview keeps that opening order, then
    /// includes the remaining rows once the same board snapshot finishes.
    mutating func completeOpeningSnapshot(_ current: [Row]) {
        var seen = Set(order)
        order.append(contentsOf: current.map(\.id).filter { seen.insert($0).inserted })
        reconcile(current)
    }

    mutating func reconcile(_ current: [Row]) {
        let byID = Dictionary(current.map { ($0.id, $0) }, uniquingKeysWith: { _, last in last })
        rows = order.compactMap { byID[$0] }
        positions = Dictionary(uniqueKeysWithValues: rows.enumerated().map { ($0.element.id, $0.offset) })
        previous.removeAll(keepingCapacity: true)
        next.removeAll(keepingCapacity: true)
        var neighbor: Row.ID?
        for id in order {
            previous[id] = neighbor
            if positions[id] != nil {
                neighbor = id
            }
        }
        neighbor = nil
        for id in order.reversed() {
            next[id] = neighbor
            if positions[id] != nil {
                neighbor = id
            }
        }
    }

    mutating func update(_ row: Row) {
        guard let index = positions[row.id], rows[index] != row else { return }
        rows[index] = row
    }

    func row(id: Row.ID) -> Row? {
        positions[id].map { rows[$0] }
    }

    func neighbor(of id: Row.ID, direction: Int) -> Row? {
        guard direction != 0, let target = direction < 0 ? previous[id] : next[id] else { return nil }
        return row(id: target)
    }
}
