// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Source-specific metadata carried by a reading without changing its primary
/// `ReadingKind`. The core owns classification and serializes this versioned
/// profile; macOS only decodes the presentation fields it understands.
struct ReadingSourceProfile: Equatable, Sendable {
    let version: UInt64
    let sourceType: ReadingSourceType
    let provider: String
    let sourceID: String
    let authorHandle: String?
    let publishedAt: String?
    let avatarAsset: String?
    let attachments: [ReadingSourceAttachment]

    static func decode(_ json: String?) -> ReadingSourceProfile? {
        guard let json,
              let data = json.data(using: .utf8),
              let payload = try? JSONDecoder().decode(Payload.self, from: data),
              payload.version > 0,
              let sourceType = ReadingSourceType(rawValue: payload.sourceType),
              let provider = nonempty(payload.provider),
              let sourceID = nonempty(payload.sourceID),
              let authorHandle = nonempty(payload.authorHandle)
        else {
            return nil
        }

        let attachments: [ReadingSourceAttachment] = payload.attachments.compactMap {
            attachment -> ReadingSourceAttachment? in
            guard let asset = nonempty(attachment.asset) else {
                return nil
            }
            return ReadingSourceAttachment(
                kind: attachment.kind,
                asset: asset,
                posterAsset: nonempty(attachment.posterAsset),
                contentType: nonempty(attachment.contentType),
                width: positive(attachment.width),
                height: positive(attachment.height),
                alt: nonempty(attachment.alt)
            )
        }

        return ReadingSourceProfile(
            version: payload.version,
            sourceType: sourceType,
            provider: provider,
            sourceID: sourceID,
            authorHandle: authorHandle,
            publishedAt: nonempty(payload.publishedAt),
            avatarAsset: nonempty(payload.avatarAsset),
            attachments: attachments
        )
    }

    var displayProvider: String {
        switch provider.lowercased() {
        case "x", "twitter": "𝕏"
        case "bluesky", "bsky": "Bluesky"
        case "mastodon": "Mastodon"
        default: provider
        }
    }

    var isXProvider: Bool {
        switch provider.lowercased() {
        case "x", "twitter": true
        default: false
        }
    }

    var displayHandle: String? {
        guard let authorHandle else { return nil }
        return authorHandle.hasPrefix("@") ? authorHandle : "@\(authorHandle)"
    }

    var primaryAttachment: ReadingSourceAttachment? {
        attachments.first { $0.mediaKind != nil }
    }
}

enum ReadingSourceType: String, Equatable, Sendable {
    case socialPost = "social_post"
}

struct ReadingSourceAttachment: Equatable, Sendable {
    let kind: String
    let asset: String
    let posterAsset: String?
    let contentType: String?
    let width: Double?
    let height: Double?
    let alt: String?

    var mediaKind: ReadingSourceAttachmentMediaKind? {
        switch kind.lowercased() {
        case "image": .image
        case "video": .video
        default: nil
        }
    }

    var previewAsset: String {
        posterAsset ?? asset
    }

    var previewIsVideo: Bool {
        mediaKind == .video && posterAsset == nil
    }
}

enum ReadingSourceAttachmentMediaKind: Equatable, Sendable {
    case image
    case video
}

private extension ReadingSourceProfile {
    struct Payload: Decodable {
        let version: UInt64
        let sourceType: String
        let provider: String
        let sourceID: String
        let authorHandle: String?
        let publishedAt: String?
        let avatarAsset: String?
        let attachments: [AttachmentPayload]

        enum CodingKeys: String, CodingKey {
            case version
            case sourceType = "source_type"
            case provider
            case sourceID = "source_id"
            case authorHandle = "author_handle"
            case publishedAt = "published_at"
            case avatarAsset = "avatar_asset"
            case attachments
        }

        init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            version = try container.decode(UInt64.self, forKey: .version)
            sourceType = try container.decode(String.self, forKey: .sourceType)
            provider = try container.decode(String.self, forKey: .provider)
            sourceID = try container.decode(String.self, forKey: .sourceID)
            authorHandle = try container.decodeIfPresent(String.self, forKey: .authorHandle)
            publishedAt = try container.decodeIfPresent(String.self, forKey: .publishedAt)
            avatarAsset = try container.decodeIfPresent(String.self, forKey: .avatarAsset)
            attachments = try container.decodeIfPresent(
                [AttachmentPayload].self,
                forKey: .attachments
            ) ?? []
        }
    }

    struct AttachmentPayload: Decodable {
        let kind: String
        let asset: String
        let posterAsset: String?
        let contentType: String?
        let width: Double?
        let height: Double?
        let alt: String?

        enum CodingKeys: String, CodingKey {
            case kind
            case asset
            case posterAsset = "poster_asset"
            case contentType = "content_type"
            case width
            case height
            case alt
        }
    }

    static func nonempty(_ value: String?) -> String? {
        guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines),
              !value.isEmpty
        else {
            return nil
        }
        return value
    }

    static func positive(_ value: Double?) -> Double? {
        guard let value, value.isFinite, value > 0 else { return nil }
        return value
    }
}
