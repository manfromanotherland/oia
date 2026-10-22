// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// The visual presentation of an image, separate from subject Tags such as interiors.
/// Missing and newer values remain unclassified on this client.
enum ReadingImageType: String, Hashable, Sendable {
    case photograph
    case other
}

/// A remembered facet of Images. Other board scopes do not inherit it.
enum ImageTypeFilter: String, CaseIterable, Hashable, Identifiable, Sendable {
    case all
    case graphics
    case photos
    case unsure

    init(savedValue: String?) {
        switch savedValue {
        case "photographs", "photography", "photos": self = .photos
        case "other", "hidePhotography", "graphics": self = .graphics
        case "unclassified", "unsure": self = .unsure
        default: self = .all
        }
    }

    var id: String { rawValue }

    static func availableCases(hasUnsureImages: Bool) -> [Self] {
        allCases.filter { $0 != .unsure || hasUnsureImages }
    }

    func availableSelection(hasUnsureImages: Bool) -> Self {
        self == .unsure && !hasUnsureImages ? .all : self
    }

    var label: String {
        switch self {
        case .all: "All"
        case .graphics: "Graphics"
        case .photos: "Photos"
        case .unsure: "Unsure"
        }
    }

    var itemTypeTerm: String? {
        switch self {
        case .all: nil
        case .graphics: "other_image"
        case .photos: "photograph"
        case .unsure: "unclassified_image"
        }
    }

    /// Presentation-only mirror for optimistic edits. Core owns actual queries.
    func contains(_ row: ReadingRow) -> Bool {
        guard row.kind == .image else { return false }
        switch self {
        case .all: return true
        case .graphics: return row.imageType == .other
        case .photos: return row.imageType == .photograph
        case .unsure: return row.imageType == nil
        }
    }
}

/// Keeps newer corrections visible when an older file-backed refresh finishes.
struct PendingImageTypeEdits {
    private struct Edit {
        let reservation: UInt64
        let ids: Set<String>
        let imageType: ReadingImageType?
        let librarySession: UInt64
    }

    private var edits: [Edit] = []
    private var generation: UInt64 = 0
    private(set) var revision: UInt64 = 0

    mutating func reserve(
        ids: Set<String>, imageType: ReadingImageType?, librarySession: UInt64
    ) -> UInt64 {
        generation &+= 1
        edits.append(Edit(
            reservation: generation, ids: ids, imageType: imageType, librarySession: librarySession
        ))
        revision &+= 1
        return generation
    }

    mutating func complete(_ reservation: UInt64) {
        guard let index = edits.firstIndex(where: { $0.reservation == reservation }) else { return }
        edits.remove(at: index)
        revision &+= 1
    }

    func apply(to row: ReadingRow, librarySession: UInt64) -> ReadingRow {
        guard row.kind == .image else { return row }
        var updated = row
        for edit in edits where edit.librarySession == librarySession && edit.ids.contains(row.id) {
            updated.imageType = edit.imageType
        }
        return updated
    }

    func hasEdits(librarySession: UInt64) -> Bool {
        edits.contains { $0.librarySession == librarySession }
    }
}
