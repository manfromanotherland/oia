// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import XCTest

final class ReadingQuickLookURLResolverTests: XCTestCase {
    private var libraryURL: URL!

    override func setUpWithError() throws {
        libraryURL = URL.temporaryDirectory
            .appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(
            at: libraryURL,
            withIntermediateDirectories: true
        )
    }

    override func tearDownWithError() throws {
        try FileManager.default.removeItem(at: libraryURL)
        libraryURL = nil
    }

    func testArticlesAndQuotesPreviewTheirSourceMarkdown() throws {
        let article = makeReadingRow(kind: .article)
        let articleURL = try writeArticle(for: article)
        XCTAssertEqual(resolve(article), articleURL)

        let quote = makeReadingRow(kind: .quote)
        XCTAssertEqual(resolve(quote), articleURL)
    }

    func testImagePreviewsItsLocalAsset() throws {
        let row = makeReadingRow(kind: .image, previewAsset: "assets/image.jpg")
        _ = try writeArticle(for: row)
        let imageURL = try writeAsset(named: "image.jpg", for: row)

        XCTAssertEqual(resolve(row), imageURL)
    }

    func testVideoPrefersMovieThenPosterThenMarkdown() throws {
        var row = makeReadingRow(
            kind: .video,
            mediaUrl: "cuttings-asset:assets/movie.mp4",
            previewAsset: "assets/poster.jpg"
        )
        let articleURL = try writeArticle(for: row)
        let posterURL = try writeAsset(named: "poster.jpg", for: row)
        let movieURL = try writeAsset(named: "movie.mp4", for: row)

        XCTAssertEqual(resolve(row), movieURL)
        try FileManager.default.removeItem(at: movieURL)
        XCTAssertEqual(resolve(row), posterURL)
        try FileManager.default.removeItem(at: posterURL)
        XCTAssertEqual(resolve(row), articleURL)

        row.mediaUrl = "https://example.com/remote.mp4"
        XCTAssertEqual(resolve(row), articleURL)
    }

    func testUnsafeOrMissingAssetsFallBackToMarkdown() throws {
        let row = makeReadingRow(kind: .image, previewAsset: "../outside.jpg")
        let articleURL = try writeArticle(for: row)
        XCTAssertEqual(resolve(row), articleURL)

        let remote = makeReadingRow(
            kind: .image,
            previewAsset: "https://example.com/image.jpg"
        )
        XCTAssertEqual(resolve(remote), articleURL)
    }

    func testMissingLibraryOrFilesCannotPreview() {
        let row = makeReadingRow(kind: .article)
        XCTAssertNil(ReadingQuickLookURLResolver.previewURL(for: row, libraryURL: nil))
        XCTAssertNil(resolve(row))
    }

    func testCopyUsesLocalFile() throws {
        let row = makeReadingRow(kind: .image, previewAsset: "assets/image.jpg")
        _ = try writeArticle(for: row)
        let imageURL = try writeAsset(named: "image.jpg", for: row)
        let pasteboard = NSPasteboard(name: NSPasteboard.Name(UUID().uuidString))
        defer { pasteboard.releaseGlobally() }

        ReadingClipboard.copyFile(row, libraryURL: libraryURL, to: pasteboard)
        XCTAssertEqual(
            (pasteboard.readObjects(forClasses: [NSURL.self]) as? [URL])?.first,
            imageURL
        )

    }

    private func resolve(_ row: ReadingRow) -> URL? {
        ReadingQuickLookURLResolver.previewURL(for: row, libraryURL: libraryURL)
    }

    @discardableResult
    private func writeArticle(for row: ReadingRow) throws -> URL {
        let folder = try readingFolder(for: row)
        let url = folder.appending(path: "article.md")
        try Data("# Preview".utf8).write(to: url)
        return url
    }

    @discardableResult
    private func writeAsset(named name: String, for row: ReadingRow) throws -> URL {
        let assets = try readingFolder(for: row).appending(path: "assets", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: assets, withIntermediateDirectories: true)
        let url = assets.appending(path: name)
        try Data([0]).write(to: url)
        return url
    }

    private func readingFolder(for row: ReadingRow) throws -> URL {
        let folder = try XCTUnwrap(
            AssetImageLoader.readingFolderURL(libraryURL: libraryURL, readingID: row.id)
        )
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }
}
