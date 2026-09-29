// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class ReadingSourceProfileTests: XCTestCase {
    func testDecodesSocialProfileAndOrderedAttachments() throws {
        let profile = try XCTUnwrap(ReadingSourceProfile.decode("""
        {
          "version": 1,
          "source_type": "social_post",
          "provider": "x",
          "source_id": "2102505743278829840",
          "author_handle": "benspringwater",
          "published_at": "2026-09-22T10:00:00Z",
          "avatar_asset": "assets/avatar.jpg",
          "attachments": [
            {
              "kind": "image",
              "asset": "assets/first.jpg",
              "content_type": "image/jpeg",
              "width": 1200,
              "height": 800,
              "alt": "A sketch"
            },
            {
              "kind": "video",
              "asset": "assets/clip.mp4",
              "poster_asset": "assets/poster.jpg",
              "content_type": "video/mp4",
              "width": 1920,
              "height": 1080
            }
          ]
        }
        """))

        XCTAssertEqual(profile.sourceType, .socialPost)
        XCTAssertEqual(profile.displayProvider, "𝕏")
        XCTAssertEqual(profile.sourceID, "2102505743278829840")
        XCTAssertEqual(profile.displayHandle, "@benspringwater")
        XCTAssertEqual(profile.avatarAsset, "assets/avatar.jpg")
        XCTAssertEqual(profile.attachments.map(\.asset), [
            "assets/first.jpg", "assets/clip.mp4"
        ])
        XCTAssertEqual(profile.primaryAttachment?.mediaKind, .image)
        XCTAssertEqual(profile.attachments.last?.posterAsset, "assets/poster.jpg")
    }

    func testIgnoresUnknownFieldsAndRetainsUnknownAttachmentKind() throws {
        let profile = try XCTUnwrap(ReadingSourceProfile.decode("""
        {
          "version": 2,
          "source_type": "social_post",
          "provider": "mastodon",
          "source_id": "instance.example/109",
          "author_handle": "@author@instance.example",
          "future_profile_field": {"nested": true},
          "attachments": [
            {
              "kind": "audio",
              "asset": "assets/audio.m4a",
              "future_attachment_field": 42
            },
            {"kind": "image", "asset": "assets/photo.webp"}
          ]
        }
        """))

        XCTAssertEqual(profile.version, 2)
        XCTAssertNil(profile.attachments.first?.mediaKind)
        XCTAssertEqual(profile.primaryAttachment?.asset, "assets/photo.webp")
    }

    func testUnknownTypeAndMalformedProfilesFallBackToOrdinaryArticle() {
        let unknownType = """
        {"version":1,"source_type":"video_page","provider":"youtube",\
        "source_id":"abc","attachments":[]}
        """
        let missingIdentity = """
        {"version":1,"source_type":"social_post","provider":"x","attachments":[]}
        """

        XCTAssertNil(ReadingSourceProfile.decode(unknownType))
        XCTAssertNil(ReadingSourceProfile.decode(missingIdentity))
        XCTAssertNil(ReadingSourceProfile.decode("not json"))
    }

    func testMissingAuthorHandleFallsBackToOrdinaryArticle() {
        XCTAssertNil(ReadingSourceProfile.decode("""
        {"version":1,"source_type":"social_post","provider":"x",\
        "source_id":"123","attachments":[]}
        """))
    }

    func testPreservesRepeatedAttachmentsInSourceOrder() throws {
        let profile = try XCTUnwrap(ReadingSourceProfile.decode("""
        {
          "version": 1,
          "source_type": "social_post",
          "provider": "x",
          "source_id": "123",
          "author_handle": "example",
          "attachments": [
            {"kind":"image","asset":"assets/same.jpg","alt":"First"},
            {"kind":"image","asset":"assets/same.jpg","alt":"Second"}
          ]
        }
        """))

        XCTAssertEqual(profile.attachments.map(\.alt), ["First", "Second"])
    }

    func testInvalidAttachmentDimensionsDoNotPoisonTheProfile() throws {
        let profile = try XCTUnwrap(ReadingSourceProfile.decode("""
        {
          "version": 1,
          "source_type": "social_post",
          "provider": "bluesky",
          "source_id": "at://did:plc:example/app.bsky.feed.post/3",
          "author_handle": "author.bsky.social",
          "attachments": [
            {"kind":"image","asset":"assets/photo.jpg","width":0,"height":-4}
          ]
        }
        """))

        XCTAssertNil(profile.primaryAttachment?.width)
        XCTAssertNil(profile.primaryAttachment?.height)
        XCTAssertEqual(profile.primaryAttachment?.intrinsicAspectRatio, 4.0 / 3.0)
    }
}
