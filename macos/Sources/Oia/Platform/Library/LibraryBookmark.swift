// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Persists and resolves a bookmark for the library folder.
/// Óia is not sandboxed, so a regular bookmark survives local builds with
/// different signing identities.
enum LibraryBookmark {
    private static let key = "libraryBookmark"

    // ── Save ──────────────────────────────────────────────────────────────

    static func save(url: URL, store: UserDefaults = .standard) throws {
        let data = try url.bookmarkData(
            options: [],
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        )
        store.set(data, forKey: key)
    }

    // ── Resolve ───────────────────────────────────────────────────────────

    /// Older security-scoped bookmarks still resolve without requesting scope.
    static func resolve(store: UserDefaults = .standard) -> URL? {
        guard let data = store.data(forKey: key) else { return nil }
        var isStale = false
        guard let url = try? URL(
            resolvingBookmarkData: data,
            options: [],
            relativeTo: nil,
            bookmarkDataIsStale: &isStale
        ) else { return nil }

        if isStale, let refreshed = try? url.bookmarkData(
            options: [],
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        ) {
            store.set(refreshed, forKey: key)
        }

        return url
    }

    // ── Clear ─────────────────────────────────────────────────────────────

    static func clear(store: UserDefaults = .standard) {
        store.removeObject(forKey: key)
    }
}
