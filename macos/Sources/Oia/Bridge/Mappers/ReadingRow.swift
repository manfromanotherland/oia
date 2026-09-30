// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// The kind of thing saved in the library. Files store a stable lowercase value;
/// the FFI exposes the same vocabulary as an exhaustive enum. The core maps a
/// legacy file with no kind to `.article` before it reaches this boundary.
enum ReadingKind: String, CaseIterable, Sendable {
    case article
    case image
    case quote
    case video

    init(_ kind: FfiReadingKind) {
        switch kind {
        case .article: self = .article
        case .image: self = .image
        case .quote: self = .quote
        case .video: self = .video
        }
    }

    var ffiKind: FfiReadingKind {
        switch self {
        case .article: .article
        case .image: .image
        case .quote: .quote
        case .video: .video
        }
    }
}

/// The highest-coverage exact sRGB cluster from derived visual analysis.
/// It is cached per device and never becomes part of the Markdown contract.
struct ReadingColor: Equatable, Sendable {
    let red: Double
    let green: Double
    let blue: Double
    let weight: Double

    init(red: Double, green: Double, blue: Double, weight: Double) {
        self.red = red
        self.green = green
        self.blue = blue
        self.weight = weight
    }

    init(_ color: FfiWeightedColor) {
        self.init(red: color.red, green: color.green, blue: color.blue, weight: color.weight)
    }
}

/// A board/search presentation snapshot of a reading, kept apart from the
/// `FfiReadingRow` boundary DTO so feature code speaks app language rather than
/// "this came from the Rust FFI" (see ADR 0001). It mirrors the boundary fields
/// exactly and as `var`, so optimistic tag edits can copy and tweak it.
/// `read`, `archived`, `favorite`, and `rating` are compatibility-only snapshots from the
/// format-v1 FFI record; current app queries and views do not use them.
struct ReadingRow: Identifiable, Equatable, Sendable {
    var id: String
    var title: String
    var url: String
    var canonicalUrl: String
    var author: String?
    var site: String?
    var savedAt: String
    /// Compatibility-only snapshots of legacy library metadata. They remain on
    /// the FFI row so older files round-trip, but no longer drive app behavior.
    var read: Bool
    var archived: Bool
    var favorite: Bool
    /// Compatibility-only legacy metadata; no longer drives app behavior.
    var rating: UInt8
    var excerpt: String?
    /// Board preview derived by the core from the saved article file.
    var cardDescription: String?
    var wordCount: UInt32?
    var lang: String?
    var tags: [String]
    /// Active machine tags within `tags`; matching user tags have precedence.
    var machineTags: [String] = []
    var kind: ReadingKind = .article
    var lightweight: Bool
    /// Core-derived board classification. Full social posts can be links while
    /// retaining their locally saved text and media.
    var isLink: Bool
    var mediaUrl: String?
    var previewAsset: String?
    var faviconAsset: String?
    var themeColor: String?
    var dominantColor: ReadingColor?
    var mediaAspectRatio: Double?
    var sourceProfile: ReadingSourceProfile?
}

extension ReadingRow {
    /// Maps the FFI boundary row into a presentation snapshot. Field-for-field
    /// today; the seam for any future presentation-only derivations (e.g. parsing
    /// `savedAt` into a `Date`) so views never see the boundary type.
    init(_ row: FfiReadingRow) {
        id = row.id
        title = row.title
        url = row.url
        canonicalUrl = row.canonicalUrl
        author = row.author
        site = row.site
        savedAt = row.savedAt
        read = row.read
        archived = row.archived
        favorite = row.favorite
        rating = row.rating
        excerpt = row.excerpt
        cardDescription = row.cardDescription
        wordCount = row.wordCount
        lang = row.lang
        tags = row.tags
        machineTags = row.machineTags
        kind = ReadingKind(row.kind)
        lightweight = row.lightweight
        isLink = row.isLink
        mediaUrl = row.mediaUrl
        previewAsset = row.previewAsset
        faviconAsset = row.faviconAsset
        themeColor = row.themeColor
        dominantColor = row.dominantColor.map(ReadingColor.init)
        mediaAspectRatio = row.mediaAspectRatio
        sourceProfile = ReadingSourceProfile.decode(row.sourceProfileJson)
    }
}

extension ReadingRow {
    /// Presentation-only mirror of a file-first tag edit until refresh settles it.
    mutating func applyTagEdit(_ tag: String, applies: Bool) {
        if applies {
            if let index = tags.firstIndex(where: { ExactTagIdentity.matches($0, tag) }) {
                if machineTags.contains(where: { ExactTagIdentity.matches($0, tag) }) {
                    tags[index] = tag
                }
            } else {
                tags.append(tag)
            }
        } else {
            tags.removeAll { ExactTagIdentity.matches($0, tag) }
        }
        machineTags.removeAll { ExactTagIdentity.matches($0, tag) }
    }

    private static let localVideoAssetPrefix = "cuttings-asset:"

    var localVideoAssetReference: String? {
        guard kind == .video,
              let mediaUrl,
              mediaUrl.hasPrefix(Self.localVideoAssetPrefix)
        else {
            return nil
        }
        return String(mediaUrl.dropFirst(Self.localVideoAssetPrefix.count))
    }

    var hasLocalVideoAsset: Bool {
        localVideoAssetReference != nil
    }

    var socialPostProfile: ReadingSourceProfile? {
        guard kind == .article,
              sourceProfile?.sourceType == .socialPost
        else {
            return nil
        }
        return sourceProfile
    }

    var isSocialPost: Bool {
        socialPostProfile != nil
    }

    var isFullArticle: Bool {
        kind == .article && !lightweight && !isSocialPost
    }

    var socialPostText: String {
        if let excerpt = excerpt?.trimmingCharacters(in: .whitespacesAndNewlines),
           !excerpt.isEmpty
        {
            return excerpt
        }
        return displayTitle
    }

    var socialPostAuthor: String {
        if let author = author?.trimmingCharacters(in: .whitespacesAndNewlines),
           !author.isEmpty
        {
            return author
        }
        if let handle = socialPostProfile?.displayHandle {
            return String(handle.drop(while: { $0 == "@" }))
        }
        return socialPostProfile?.displayProvider ?? "Social post"
    }
}
