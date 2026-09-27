// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class LibraryBookmarkTests: XCTestCase {
    func testLibraryBookmarkRestoresFromAnIsolatedPreferencesStore() throws {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("oia-bookmark-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }

        let suite = "is.edmundo.cuttings.bookmark-test.\(UUID().uuidString)"
        let store = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { store.removePersistentDomain(forName: suite) }

        try LibraryBookmark.save(url: folder, store: store)
        XCTAssertEqual(LibraryBookmark.resolve(store: store)?.standardizedFileURL, folder.standardizedFileURL)
    }
}
