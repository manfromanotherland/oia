// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

extension ReadingRowMapperTests {
    func testBlueskyPostMapsToLinkWithLocalSocialPresentation() {
        let row = blueskyRow()

        XCTAssertTrue(row.isLink)
        XCTAssertTrue(LibraryScope.links.contains(row))
        XCTAssertFalse(LibraryScope.articles.contains(row))
        XCTAssertFalse(row.isFullArticle)
        XCTAssertTrue(row.hasLocalSocialPreview)
        XCTAssertEqual(row.socialPostAuthor, "Montoulieu")
        XCTAssertEqual(row.socialPostText, "A saved Bluesky post.")
        XCTAssertEqual(row.boardGeometryKey, .social("A saved Bluesky post.", 16.0 / 9.0))
    }

    func testBlueskyPostMapsOrderedLocalMediaAndPoster() throws {
        let profile = try XCTUnwrap(blueskyRow().socialPostProfile)

        XCTAssertTrue(profile.isBlueskyProvider)
        XCTAssertFalse(profile.isXProvider)
        XCTAssertEqual(profile.displayHandle, "@montoulieu.dev")
        XCTAssertEqual(profile.avatarAsset, "assets/avatar.jpg")
        XCTAssertEqual(profile.attachments.map(\.asset), ["assets/clip.mp4", "assets/photo.jpg"])
        XCTAssertEqual(profile.primaryAttachment?.previewAsset, "assets/poster.jpg")
        XCTAssertEqual(profile.primaryAttachment?.mediaKind, .video)
    }

    private func blueskyRow() -> ReadingRow {
        var ffi = sampleFfiRow()
        ffi.kind = .article
        ffi.lightweight = false
        ffi.isLink = true
        ffi.url = "https://bsky.app/profile/montoulieu.dev/post/3muu5higixs2i"
        ffi.author = "Montoulieu"
        ffi.excerpt = "A saved Bluesky post."
        ffi.featuredHighlightText = nil
        ffi.sourceProfileJson = """
        {
          "version":1,"source_type":"social_post","provider":"bluesky",
          "source_id":"at://did:plc:example/app.bsky.feed.post/3muu5higixs2i",
          "author_handle":"montoulieu.dev","avatar_asset":"assets/avatar.jpg",
          "attachments":[
            {"kind":"video","asset":"assets/clip.mp4","poster_asset":"assets/poster.jpg",
             "width":1920,"height":1080},
            {"kind":"image","asset":"assets/photo.jpg","width":1200,"height":800}
          ]
        }
        """
        return ReadingRow(ffi)
    }
}
