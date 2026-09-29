// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// The native toolbar segments select one core-backed board scope at a time.
final class BoardScopeJourney: UITestCase {
    func testPreviouslyCapturedTweetIsALinkWithLocalDetail() throws {
        let id = TestULID.make(306)
        let tweet = ArticleFixture(
            id: id,
            url: "https://x.com/example/status/123",
            title: "A saved tweet",
            savedAt: Date(timeIntervalSince1970: 1_700_000_000),
            excerpt: "A locally captured tweet.",
            sourceProfileYAML: """
              version: 1
              source_type: social_post
              provider: x
              source_id: "123"
              author_handle: example
              attachments: []
            """,
            body: "A locally captured tweet.\n"
        )
        try launchApp(articles: [tweet])

        list.selectScope("Links")
        XCTAssertTrue(list.waitForRowCount(1), "captured tweet appears under Links")
        XCTAssertTrue(list.row(id).waitExists())

        list.open(id)
        XCTAssertTrue(app.byId(A11y.Detail.close).waitExists(), "saved tweet opens local detail")
        app.byId(A11y.Detail.close).clickWhenReady()

        list.selectScope("Articles")
        XCTAssertTrue(list.waitForRowCount(0), "captured tweet is absent from Articles")
    }

    func testLabeledSegmentsSelectAllSixScopes() throws {
        let savedAt = Date(timeIntervalSince1970: 1_700_000_000)
        let readings: [(scope: String, id: String, title: String, kind: String?, lightweight: Bool)] = [
            ("Images", TestULID.make(301), "Image card", "image", false),
            ("Videos", TestULID.make(302), "Video card", "video", false),
            ("Articles", TestULID.make(303), "Article card", nil, false),
            ("Links", TestULID.make(304), "Link card", nil, true),
            ("Quotes", TestULID.make(305), "Quote card", "quote", false)
        ]
        let fixtures = readings.map { reading in
            ArticleFixture(
                id: reading.id,
                url: "https://example.com/board-scope/\(reading.id)",
                title: reading.title,
                savedAt: savedAt,
                kind: reading.kind,
                lightweight: reading.lightweight
            )
        }
        try launchApp(articles: fixtures)

        let allIDs = Set(readings.map { $0.id })
        XCTAssertTrue(list.waitForRowCount(readings.count), "All shows every saved kind")
        XCTAssertEqual(Set(list.orderedRowIds), allIDs)
        XCTAssertTrue(list.filterGroup.waitExists(), "Labeled scope control is visible")

        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Six labeled board scopes · All"
        attachment.lifetime = .keepAlways
        add(attachment)

        for reading in readings {
            list.selectScope(reading.scope)
            XCTAssertTrue(
                wait { list.orderedRowIds == [reading.id] },
                "\(reading.scope) should show only its matching card"
            )
        }

        list.showAll()
        XCTAssertTrue(
            wait { Set(list.orderedRowIds) == allIDs },
            "All restores the complete mixed board"
        )
    }
}
