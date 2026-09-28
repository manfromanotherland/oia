// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import XCTest

/// Paste-to-save contract: the board imports plain text exactly once, while the
/// same shortcut keeps its native editing behavior when the search field owns
/// focus.
final class PastingToSaveJourney: UITestCase {
    func testPlainTextPasteSavesDeduplicatesAndRespectsSearchFocus() throws {
        try launchApp()

        let cutting = "A durable pasted cutting for the ingestion journey."
        XCTAssertTrue(list.emptyState.waitExists(), "empty board is ready")

        // 1. Paste plain text onto the empty board. The real app/core path adds
        //    one visible row and one article.md to the isolated library.
        paste(cutting, afterClicking: list.emptyState)
        XCTAssertTrue(list.waitForRowCount(1), "paste creates exactly one row")
        XCTAssertTrue(wait { self.articleFiles.count == 1 }, "paste creates one on-disk reading")

        let savedRowIDs = list.orderedRowIds
        let savedArticle = try XCTUnwrap(articleFiles.first)
        try XCTAssertTrue(
            String(contentsOf: savedArticle, encoding: .utf8).contains(cutting),
            "the saved reading contains the pasted text"
        )

        // 2. Re-pasting the identical value resolves as a duplicate. Wait for
        //    that acknowledgement so the unchanged-count assertions cannot race
        //    the asynchronous item-provider/core work.
        paste(cutting)
        XCTAssertTrue(
            wait { self.app.byId(A11y.Save.notice).label.contains("Already saved") },
            "duplicate paste is acknowledged"
        )
        XCTAssertTrue(list.waitForRowCount(1), "duplicate leaves exactly one row")
        XCTAssertEqual(list.orderedRowIds, savedRowIDs)
        XCTAssertEqual(articleFiles, [savedArticle], "duplicate leaves exactly one on-disk reading")

        // 3. Once search owns focus, ⌘V belongs to its field editor. Waiting
        //    for the previous notice to leave lets any accidental import surface
        //    a fresh notice during the assertion window below.
        XCTAssertTrue(app.byId(A11y.Save.notice).waitDisappears(6), "duplicate notice clears")
        let query = "search-only-paste"
        list.pasteSearch(query)
        XCTAssertTrue(wait { (self.list.searchField.value as? String) == query }, "search receives paste")
        XCTAssertTrue(list.waitForRowCount(0), "pasted search filters the existing row")
        XCTAssertFalse(
            app.byId(A11y.Save.notice).waitForExistence(timeout: 1),
            "search paste does not trigger board ingestion"
        )
        XCTAssertEqual(articleFiles, [savedArticle], "search paste leaves the library unchanged")
    }

    // swiftlint:disable:next function_body_length
    func testVideoFilePasteCopiesPlaysAndDeduplicates() throws {
        try launchApp()

        let sourceURL = library.root.appendingPathComponent("Finder video.mp4")
        let videoData = try XCTUnwrap(
            Data(base64Encoded: Self.sampleVideoBase64, options: .ignoreUnknownCharacters)
        )
        try videoData.write(to: sourceURL)
        XCTAssertTrue(list.emptyState.waitExists(), "empty board is ready")

        // NSURL writes exactly `public.file-url`, matching a copied Finder file.
        // The app must copy the movie into the reading rather than retaining the
        // disposable source path.
        paste(sourceURL, afterClicking: list.emptyState)
        XCTAssertTrue(
            wait {
                let notice = self.app.byId(A11y.Save.notice)
                return notice.exists && notice.label.contains("Saved to Óia")
            },
            "video paste is acknowledged"
        )
        XCTAssertTrue(list.waitForRowCount(1), "video paste creates exactly one row")
        XCTAssertTrue(wait { self.articleFiles.count == 1 }, "video paste creates one on-disk reading")

        let savedArticle = try XCTUnwrap(articleFiles.first)
        let rowID = savedArticle.deletingLastPathComponent().lastPathComponent
        let article = try String(contentsOf: savedArticle, encoding: .utf8)
        XCTAssertTrue(article.contains("kind: video"), "saved reading is a video")
        XCTAssertTrue(
            article.contains("media_url: cuttings-asset:assets/"),
            "saved reading points at its local movie asset"
        )
        XCTAssertFalse(article.contains(sourceURL.path), "source path is not persisted")
        XCTAssertTrue(list.row(rowID, contains: "Finder video"), "video card uses the source filename")

        let savedAsset = try XCTUnwrap(assetFiles(for: savedArticle).first)
        XCTAssertEqual(assetFiles(for: savedArticle).count, 1, "reading contains one movie asset")
        XCTAssertEqual(try Data(contentsOf: savedAsset), videoData, "saved movie bytes match the source")

        // The same Finder file is content-addressed and must not create another
        // card, article, or asset.
        paste(sourceURL)
        XCTAssertTrue(
            wait {
                let notice = self.app.byId(A11y.Save.notice)
                return notice.exists && notice.label.contains("Already saved")
            },
            "duplicate video paste is acknowledged"
        )
        XCTAssertTrue(list.waitForRowCount(1), "duplicate leaves exactly one row")
        XCTAssertEqual(articleFiles, [savedArticle], "duplicate leaves exactly one on-disk reading")
        XCTAssertEqual(assetFiles(for: savedArticle), [savedAsset], "duplicate leaves exactly one movie asset")

        // Removing the original proves the card resolves and validates the
        // library-owned copy, not the Finder URL used during import.
        try FileManager.default.removeItem(at: sourceURL)
        XCTAssertFalse(FileManager.default.fileExists(atPath: sourceURL.path))
        list.open(rowID)
        XCTAssertTrue(
            app.byId(A11y.Detail.videoPlayer).waitForExistence(timeout: 12),
            "saved local video becomes playable after its source is deleted"
        )
        XCTAssertFalse(app.byId(A11y.Detail.videoUnavailable).exists)
        app.byId(A11y.Detail.videoPlayer).click()
        keyboard.escape()
        XCTAssertTrue(app.byId(A11y.List.rows).waitExists(), "Escape closes the video detail")
    }

    private var articleFiles: [URL] {
        guard let enumerator = FileManager.default.enumerator(
            at: library.articlesDir,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        ) else { return [] }

        return enumerator.compactMap { $0 as? URL }
            .filter { $0.lastPathComponent == "article.md" }
            .sorted { $0.path < $1.path }
    }

    private func assetFiles(for articleURL: URL) -> [URL] {
        let assetsURL = articleURL.deletingLastPathComponent()
            .appendingPathComponent("assets", isDirectory: true)
        return (try? FileManager.default.contentsOfDirectory(
            at: assetsURL,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        ))?.sorted { $0.path < $1.path } ?? []
    }

    private func paste(_ text: String, afterClicking target: XCUIElement? = nil) {
        NSPasteboard.general.clearContents()
        XCTAssertTrue(NSPasteboard.general.setString(text, forType: .string))
        target?.clickWhenReady()
        app.typeKey("v", modifierFlags: .command)
    }

    private func paste(_ fileURL: URL, afterClicking target: XCUIElement? = nil) {
        NSPasteboard.general.clearContents()
        XCTAssertTrue(NSPasteboard.general.writeObjects([fileURL as NSURL]))
        XCTAssertTrue(NSPasteboard.general.types?.contains(.fileURL) == true)
        target?.clickWhenReady()
        app.typeKey("v", modifierFlags: .command)
    }

    /// A deterministic one-second, 64x36 H.264 MP4 used as a real AVFoundation
    /// input while keeping the UI-test target self-contained.
    private static let sampleVideoBase64 = """
    AAAAIGZ0eXBpc29tAAACAGlzb21pc28yYXZjMW1wNDEAAAN3bW9vdgAAAGxtdmhkAAAAAAAAAAAAAAAAAAAD6AAAA+gAAQAAAQAA
    AAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAA
    AqF0cmFrAAAAXHRraGQAAAADAAAAAAAAAAAAAAABAAAAAAAAA+gAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAABAAAA
    AAAAAAAAAAAAAABAAAAAAEAAAAAkAAAAAAAkZWR0cwAAABxlbHN0AAAAAAAAAAEAAAPoAAAQAAABAAAAAAIZbWRpYQAAACBtZGhk
    AAAAAAAAAAAAAAAAAAAoAAAAKABVxAAAAAAALWhkbHIAAAAAAAAAAHZpZGUAAAAAAAAAAAAAAABWaWRlb0hhbmRsZXIAAAABxG1p
    bmYAAAAUdm1oZAAAAAEAAAAAAAAAAAAAACRkaW5mAAAAHGRyZWYAAAAAAAAAAQAAAAx1cmwgAAAAAQAAAYRzdGJsAAAAwHN0c2QA
    AAAAAAAAAQAAALBhdmMxAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAEAAJABIAAAASAAAAAAAAAABFUxhdmM2Mi4yOC4xMDIgbGli
    eDI2NAAAAAAAAAAAAAAAGP//AAAANmF2Y0MBZAAK/+EAGWdkAAqs2UR/nwEQAAADABAAAAMAoPEiWWABAAZo6+PLIsD9+PgAAAAA
    EHBhc3AAAAABAAAAAQAAABRidHJ0AAAAAAAAGFgAAAAAAAAAGHN0dHMAAAAAAAAAAQAAAAUAAAgAAAAAFHN0c3MAAAAAAAAAAQAA
    AAEAAAA4Y3R0cwAAAAAAAAAFAAAAAQAAEAAAAAABAAAoAAAAAAEAABAAAAAAAQAAAAAAAAABAAAIAAAAABxzdHNjAAAAAAAAAAEA
    AAABAAAABQAAAAEAAAAoc3RzegAAAAAAAAAAAAAABQAAAtkAAAAOAAAADAAAAAwAAAAMAAAAFHN0Y28AAAAAAAAAAQAAA6cAAABi
    dWR0YQAAAFptZXRhAAAAAAAAACFoZGxyAAAAAAAAAABtZGlyYXBwbAAAAAAAAAAAAAAAAC1pbHN0AAAAJal0b28AAAAdZGF0YQAA
    AAEAAAAATGF2ZjYyLjEyLjEwMgAAAAhmcmVlAAADE21kYXQAAAKtBgX//6ncRem95tlIt5Ys2CDZI+7veDI2NCAtIGNvcmUgMTY1
    IHIzMjIyIGIzNTYwNWEgLSBILjI2NC9NUEVHLTQgQVZDIGNvZGVjIC0gQ29weWxlZnQgMjAwMy0yMDI1IC0gaHR0cDovL3d3dy52
    aWRlb2xhbi5vcmcveDI2NC5odG1sIC0gb3B0aW9uczogY2FiYWM9MSByZWY9MyBkZWJsb2NrPTE6MDowIGFuYWx5c2U9MHgzOjB4
    MTEzIG1lPWhleCBzdWJtZT03IHBzeT0xIHBzeV9yZD0xLjAwOjAuMDAgbWl4ZWRfcmVmPTEgbWVfcmFuZ2U9MTYgY2hyb21hX21l
    PTEgdHJlbGxpcz0xIDh4OGRjdD0xIGNxbT0wIGRlYWR6b25lPTIxLDExIGZhc3RfcHNraXA9MSBjaHJvbWFfcXBfb2Zmc2V0PS0y
    IHRocmVhZHM9MSBsb29rYWhlYWRfdGhyZWFkcz0xIHNsaWNlZF90aHJlYWRzPTAgbnI9MCBkZWNpbWF0ZT0xIGludGVybGFjZWQ9
    MCBibHVyYXlfY29tcGF0PTAgY29uc3RyYWluZWRfaW50cmE9MCBiZnJhbWVzPTMgYl9weXJhbWlkPTIgYl9hZGFwdD0xIGJfYmlh
    cz0wIGRpcmVjdD0xIHdlaWdodGI9MSBvcGVuX2dvcD0wIHdlaWdodHA9MiBrZXlpbnQ9MjUwIGtleWludF9taW49NSBzY2VuZWN1
    dD00MCBpbnRyYV9yZWZyZXNoPTAgcmNfbG9va2FoZWFkPTQwIHJjPWNyZiBtYnRyZWU9MSBjcmY9MjMuMCBxY29tcD0wLjYwIHFw
    bWluPTAgcXBtYXg9NjkgcXBzdGVwPTQgaXBfcmF0aW89MS40MCBhcT0xOjEuMDAAgAAAACRliIQAEv/+6Mn8yykuZ1CuR5dIp2nH
    MI9HSP+uQdmS8ghwWLEAAAAKQZokbEP//qnToAAAAAhBnkJ4gh8F9QAAAAgBnmF0Q/8JWAAAAAgBnmNqQ/8JWQ==
    """
}
