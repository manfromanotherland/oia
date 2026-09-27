// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// The durable scopes exposed by the board toolbar. Read, archive, and rating
/// metadata remains in the file format for compatibility but no longer
/// participates in browsing.
enum LibraryScope: String, CaseIterable, Hashable, Identifiable {
    case all, images, videos, articles, links, quotes

    var id: String {
        rawValue
    }

    var label: String {
        switch self {
        case .all: "All"
        case .images: "Images"
        case .videos: "Videos"
        case .articles: "Articles"
        case .links: "Links"
        case .quotes: "Quotes"
        }
    }

    var icon: String {
        switch self {
        case .all: "asterisk"
        case .images: "photo"
        case .videos: "play.rectangle"
        case .articles: "newspaper"
        case .links: "link"
        case .quotes: "quote.opening"
        }
    }

    var previous: Self {
        adjacent(by: -1)
    }

    var next: Self {
        adjacent(by: 1)
    }

    /// Whether `row` belongs in this library scope. `.all` deliberately includes
    /// cards carrying a legacy archived flag: archive remains readable metadata
    /// for format compatibility, but no longer changes presentation behavior.
    func contains(_ row: ReadingRow) -> Bool {
        switch self {
        case .all: true
        case .images: row.kind == .image
        case .videos: row.kind == .video
        case .articles: row.kind == .article && !row.lightweight
        case .links: row.kind == .article && row.lightweight
        case .quotes: row.kind == .quote
        }
    }

    private func adjacent(by offset: Int) -> Self {
        let scopes = Self.allCases
        guard let index = scopes.firstIndex(of: self) else { return self }
        let wrappedIndex = (index + offset + scopes.count) % scopes.count
        return scopes[wrappedIndex]
    }
}
