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
            productJson: nil,
            linkCategory: nil,
            featuredHighlightText: "A saved passage from the source.",
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
            tags: ["rust", "local-first", "software"],
            machineTags: ["software"]
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
        XCTAssertEqual(row.machineTags, ffi.machineTags)
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
        XCTAssertEqual(row.featuredHighlightText, ffi.featuredHighlightText)
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
        ffi.productJson = nil
        ffi.featuredHighlightText = nil
        ffi.dominantColor = nil
        ffi.mediaAspectRatio = nil
        ffi.author = nil
        ffi.site = nil
        ffi.excerpt = nil
        ffi.cardDescription = nil
        ffi.wordCount = nil
        ffi.lang = nil
        ffi.tags = []
        ffi.machineTags = []
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
        XCTAssertNil(row.product)
        XCTAssertNil(row.linkCategory)
        XCTAssertNil(row.featuredHighlight)
        XCTAssertNil(row.dominantColor)
        XCTAssertNil(row.mediaAspectRatio)
        XCTAssertTrue(row.tags.isEmpty)
        XCTAssertTrue(row.machineTags.isEmpty)
    }

    func testMapsQuoteKind() {
        var ffi = sampleFfiRow()
        ffi.kind = .quote
        XCTAssertEqual(ReadingRow(ffi).kind, .quote)
    }

    func testFeaturedPassageUsesArticleIdentityAndIgnoresStaleNonArticleMetadata() {
        var ffi = sampleFfiRow()
        ffi.kind = .article
        let article = ReadingRow(ffi)
        XCTAssertEqual(article.featuredHighlight, ffi.featuredHighlightText)
        XCTAssertEqual(article.id, ffi.id)
        XCTAssertEqual(article.boardGeometryKey, .highlight("A saved passage from the source."))

        ffi.kind = .quote
        XCTAssertNil(ReadingRow(ffi).featuredHighlight)
    }

    func testEmptyFeaturedPassageDoesNotOverrideArticleCard() {
        var ffi = sampleFfiRow()
        ffi.kind = .article
        ffi.sourceProfileJson = nil
        ffi.featuredHighlightText = " \n\t "
        let row = ReadingRow(ffi)
        XCTAssertNil(row.featuredHighlight)
        if case .highlight = row.boardGeometryKey {
            XCTFail("Whitespace cannot become a highlighted passage card")
        }
    }

    func testMapsProductPriceAndKeepsFullCaptureOutOfArticlePresentation() {
        var ffi = sampleFfiRow()
        ffi.kind = .article
        ffi.lightweight = false
        ffi.sourceProfileJson = nil
        ffi.productJson = """
        {"version":1,"price":"135.00","currency":"EUR","captured_at":"2026-10-10T12:00:00Z"}
        """
        let row = ReadingRow(ffi)
        XCTAssertTrue(row.isProduct)
        XCTAssertFalse(row.isFullArticle)
        XCTAssertEqual(row.product?.price, "135.00")
        XCTAssertEqual(row.product?.currency, "EUR")
        XCTAssertEqual(row.product?.formattedPrice(locale: Locale(identifier: "en_IE")), "€135")
    }

    func testBookCategoryTakesPrecedenceOverProductAndPreservesPriceSnapshot() {
        var ffi = sampleFfiRow()
        ffi.kind = .article
        ffi.lightweight = false
        ffi.sourceProfileJson = nil
        ffi.featuredHighlightText = nil
        ffi.linkCategory = "book"
        ffi.productJson = "{\"version\":1,\"price\":\"18.99\",\"currency\":\"GBP\"}"
        let row = ReadingRow(ffi)
        XCTAssertEqual(row.culturalLinkCategory, .book)
        XCTAssertFalse(row.isProduct)
        XCTAssertFalse(row.isFullArticle)
        XCTAssertEqual(row.product?.price, "18.99")
        XCTAssertEqual(row.boardGeometryKey, .culturalLink(.book, 2 / 3))
    }

    func testMusicUsesSquareCoverWithSpaceForRecordAndHighlightKeepsPriority() {
        var ffi = sampleFfiRow()
        ffi.kind = .article
        ffi.sourceProfileJson = nil
        ffi.linkCategory = "music"
        let row = ReadingRow(ffi)
        XCTAssertEqual(row.culturalCardAspectRatio, 1 / 1.18)
        XCTAssertEqual(row.boardGeometryKey, .highlight("A saved passage from the source."))
        ffi.featuredHighlightText = nil
        XCTAssertEqual(ReadingRow(ffi).boardGeometryKey, .culturalLink(.music, Double(1 / 1.18)))
    }

    func testUnknownCategoriesAndNonArticleMetadataUseOrdinaryPresentation() {
        var ffi = sampleFfiRow()
        ffi.linkCategory = "book"
        XCTAssertNil(ReadingRow(ffi).culturalLinkCategory)
        ffi.kind = .article
        ffi.linkCategory = "future-category"
        XCTAssertNil(ReadingRow(ffi).linkCategory)
    }

    func testProductPriceHandlesFractionsFromPricesAndMissingPrices() {
        let product = ReadingProductMetadata.decode("""
        {"version":1,"price":"129.95","currency":"EUR","price_is_from":true}
        """)
        XCTAssertEqual(product?.formattedPrice(locale: Locale(identifier: "en_IE")), "From €129.95")
        XCTAssertNotNil(ReadingProductMetadata.decode("""
        {"version":1,"price_is_from":false}
        """))
        XCTAssertNil(ReadingProductMetadata.decode("""
        {"version":1,"price_is_from":false}
        """)?.formattedPrice())
        XCTAssertNil(ReadingProductMetadata.decode("""
        {"version":2,"price":"1","currency":"EUR","price_is_from":false}
        """))
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
