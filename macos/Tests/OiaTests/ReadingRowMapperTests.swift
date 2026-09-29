// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// `ReadingRow(_: FfiReadingRow)` is the single crossing from the FFI boundary
/// DTO to the presentation snapshot. These tests pin that every field carries
/// across intact, that optionals preserve nil, and that equality tracks fields.
final class ReadingRowMapperTests: XCTestCase {
    /// An FFI row with a distinct value in every field, so a miswired assignment
    /// (say `url` ← `canonicalUrl`) surfaces as a mismatch.
    private func sampleFfiRow() -> FfiReadingRow {
        FfiReadingRow(
            id: "01JREADING000000000000000000",
            title: "The Title",
            kind: .video,
            lightweight: true,
            isLink: true,
            hasNote: true,
            url: "https://example.com/article",
            mediaUrl: "https://cdn.example.com/video.mp4",
            previewAsset: "assets/poster.jpg",
            faviconAsset: "assets/favicon.ico",
            themeColor: "#123456",
            sourceProfileJson: """
            {"version":1,"source_type":"social_post","provider":"x",\
            "source_id":"1890000000000000000","author_handle":"@ada",\
            "attachments":[]}
            """,
            dominantColor: FfiWeightedColor(red: 0.1, green: 0.2, blue: 0.3, weight: 0.6),
            mediaAspectRatio: 16.0 / 9.0,
            canonicalUrl: "https://example.com/article?canonical",
            author: "Ada Lovelace",
            site: "example.com",
            savedAt: "2026-03-04T05:06:07Z",
            read: true,
            archived: true,
            favorite: true,
            rating: 4,
            excerpt: "A short excerpt.",
            cardDescription: "The card description.",
            wordCount: 1234,
            lang: "en",
            tags: ["rust", "local-first"]
        )
    }

    func testMapsEveryFieldAcross() {
        let ffi = sampleFfiRow()
        let row = ReadingRow(ffi)
        XCTAssertEqual(row.id, ffi.id)
        XCTAssertEqual(row.title, ffi.title)
        XCTAssertEqual(row.url, ffi.url)
        XCTAssertEqual(row.canonicalUrl, ffi.canonicalUrl)
        XCTAssertEqual(row.author, ffi.author)
        XCTAssertEqual(row.site, ffi.site)
        XCTAssertEqual(row.savedAt, ffi.savedAt)
        XCTAssertEqual(row.read, ffi.read)
        XCTAssertEqual(row.archived, ffi.archived)
        XCTAssertEqual(row.favorite, ffi.favorite)
        XCTAssertEqual(row.rating, ffi.rating)
        XCTAssertEqual(row.excerpt, ffi.excerpt)
        XCTAssertEqual(row.cardDescription, ffi.cardDescription)
        XCTAssertEqual(row.wordCount, ffi.wordCount)
        XCTAssertEqual(row.lang, ffi.lang)
        XCTAssertEqual(row.tags, ffi.tags)
        XCTAssertEqual(row.kind, .video)
        XCTAssertEqual(row.lightweight, ffi.lightweight)
        XCTAssertEqual(row.isLink, ffi.isLink)
        XCTAssertEqual(row.mediaUrl, ffi.mediaUrl)
        XCTAssertEqual(row.previewAsset, ffi.previewAsset)
        XCTAssertEqual(row.faviconAsset, ffi.faviconAsset)
        XCTAssertEqual(row.themeColor, ffi.themeColor)
        XCTAssertEqual(row.sourceProfile?.sourceType, .socialPost)
        XCTAssertEqual(row.sourceProfile?.provider, "x")
        XCTAssertEqual(row.sourceProfile?.sourceID, "1890000000000000000")
        XCTAssertEqual(row.sourceProfile?.displayHandle, "@ada")
        XCTAssertEqual(row.dominantColor?.red, ffi.dominantColor?.red)
        XCTAssertEqual(row.dominantColor?.green, ffi.dominantColor?.green)
        XCTAssertEqual(row.dominantColor?.blue, ffi.dominantColor?.blue)
        XCTAssertEqual(row.dominantColor?.weight, ffi.dominantColor?.weight)
        XCTAssertEqual(row.mediaAspectRatio, ffi.mediaAspectRatio)
    }

    func testOptionalFieldsPreserveNil() {
        var ffi = sampleFfiRow()
        ffi.kind = .article
        ffi.lightweight = false
        ffi.isLink = false
        ffi.mediaUrl = nil
        ffi.previewAsset = nil
        ffi.faviconAsset = nil
        ffi.themeColor = nil
        ffi.sourceProfileJson = nil
        ffi.dominantColor = nil
        ffi.mediaAspectRatio = nil
        ffi.author = nil
        ffi.site = nil
        ffi.excerpt = nil
        ffi.cardDescription = nil
        ffi.wordCount = nil
        ffi.lang = nil
        ffi.tags = []
        let row = ReadingRow(ffi)
        XCTAssertNil(row.author)
        XCTAssertNil(row.site)
        XCTAssertNil(row.excerpt)
        XCTAssertNil(row.cardDescription)
        XCTAssertNil(row.wordCount)
        XCTAssertNil(row.lang)
        XCTAssertEqual(row.kind, .article)
        XCTAssertFalse(row.lightweight)
        XCTAssertFalse(row.isLink)
        XCTAssertNil(row.mediaUrl)
        XCTAssertNil(row.previewAsset)
        XCTAssertNil(row.faviconAsset)
        XCTAssertNil(row.themeColor)
        XCTAssertNil(row.sourceProfile)
        XCTAssertNil(row.dominantColor)
        XCTAssertNil(row.mediaAspectRatio)
        XCTAssertTrue(row.tags.isEmpty)
    }

    func testMapsQuoteKind() {
        var ffi = sampleFfiRow()
        ffi.kind = .quote
        XCTAssertEqual(ReadingRow(ffi).kind, .quote)
    }

    func testEqualityTracksFields() {
        let base = ReadingRow(sampleFfiRow())
        XCTAssertEqual(base, ReadingRow(sampleFfiRow()))
        var favoriteFlipped = base
        favoriteFlipped.favorite.toggle()
        XCTAssertNotEqual(base, favoriteFlipped)
        var linkChanged = base
        linkChanged.isLink.toggle()
        XCTAssertNotEqual(base, linkChanged)
        var mediaRatioChanged = base
        mediaRatioChanged.mediaAspectRatio = 4.0 / 3.0
        XCTAssertNotEqual(base, mediaRatioChanged)
        var sourceProfileChanged = base
        sourceProfileChanged.sourceProfile = nil
        XCTAssertNotEqual(base, sourceProfileChanged)
        var dominantColorChanged = base
        dominantColorChanged.dominantColor = ReadingColor(
            red: 0.9, green: 0.8, blue: 0.7, weight: 0.6
        )
        XCTAssertNotEqual(base, dominantColorChanged)
    }
}
