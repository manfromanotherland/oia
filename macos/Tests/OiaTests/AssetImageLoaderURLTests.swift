// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import XCTest

final class AssetImageLoaderURLTests: XCTestCase {
    func testReadingFolderKeepsDirectoryHintWhenSyncedFilesAreAbsent() throws {
        let library = URL.temporaryDirectory
            .appending(path: "oia-absent-library-\(UUID())", directoryHint: .isDirectory)
        let readingID = String(repeating: "a", count: 64)

        let folder = try XCTUnwrap(
            AssetImageLoader.readingFolderURL(libraryURL: library, readingID: readingID)
        )

        XCTAssertFalse(FileManager.default.fileExists(atPath: library.path))
        XCTAssertEqual(folder.path, library.path + "/articles/aa/" + readingID)
        XCTAssertTrue(folder.hasDirectoryPath)
    }

    func testLocalAssetIsAFileDirectlyUnderItsReadingFolder() throws {
        let library = URL.temporaryDirectory
            .appending(path: "oia-absent-library-\(UUID())", directoryHint: .isDirectory)
        let folder = try XCTUnwrap(
            AssetImageLoader.readingFolderURL(libraryURL: library, readingID: "abcdef")
        )

        let asset = try XCTUnwrap(
            AssetImageLoader.localURL(source: "assets/image.jpg", assetBaseURL: folder)
        )

        XCTAssertEqual(asset.path, library.path + "/articles/ab/abcdef/assets/image.jpg")
        XCTAssertFalse(asset.hasDirectoryPath)
    }

    func testLocalAssetRejectsReferencesOutsideItsAssetsDirectory() {
        let folder = URL(fileURLWithPath: "/tmp/oia/articles/ab/abcdef", isDirectory: true)
        for source in [
            "", "image.jpg", "assets/", "assets/.", "assets/..",
            "assets/../secret", "assets/nested/image.jpg", "/assets/image.jpg",
            "https://example.com/image.jpg"
        ] {
            XCTAssertNil(
                AssetImageLoader.localURL(source: source, assetBaseURL: folder),
                source
            )
        }
        XCTAssertNil(AssetImageLoader.localURL(source: "assets/image.jpg", assetBaseURL: nil))
        XCTAssertNil(AssetImageLoader.readingFolderURL(libraryURL: nil, readingID: "abcdef"))
        XCTAssertNil(AssetImageLoader.readingFolderURL(libraryURL: folder, readingID: ""))
    }
}
