// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Builds a `ReadingRow` for logic tests, defaulting every field so a test sets
/// only the flags it exercises. The non-flag fields are fixed placeholders; the
/// library-scope and content-length logic never reads them.
func makeReadingRow(
    read: Bool = false,
    archived: Bool = false,
    favorite: Bool = false,
    lightweight: Bool = false,
    wordCount: UInt32? = nil,
    kind: ReadingKind = .article,
    mediaUrl: String? = nil,
    previewAsset: String? = nil,
    dominantColor: ReadingColor? = nil,
    mediaAspectRatio: Double? = nil,
    sourceProfile: ReadingSourceProfile? = nil
) -> ReadingRow {
    ReadingRow(
        id: "01JQ0000000000000000000000",
        title: "Placeholder",
        url: "https://example.com/a",
        canonicalUrl: "https://example.com/a",
        author: nil,
        site: nil,
        savedAt: "2026-01-01T00:00:00Z",
        read: read,
        archived: archived,
        favorite: favorite,
        rating: 0,
        excerpt: nil,
        cardDescription: nil,
        wordCount: wordCount,
        lang: nil,
        tags: [],
        kind: kind,
        lightweight: lightweight,
        mediaUrl: mediaUrl,
        previewAsset: previewAsset,
        faviconAsset: nil,
        themeColor: nil,
        dominantColor: dominantColor,
        mediaAspectRatio: mediaAspectRatio,
        sourceProfile: sourceProfile
    )
}
