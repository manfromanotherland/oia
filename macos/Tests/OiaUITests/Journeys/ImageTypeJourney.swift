// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// Pixel classification and manual corrections stay separate from subject Tags.
final class ImageTypeJourney: UITestCase {
    func testImageFiltersCorrectionsAndScopePersistence() throws {
        let photo = TestULID.make(701)
        let graphic = TestULID.make(702)
        let unknown = TestULID.make(703)
        let article = TestULID.make(704)
        let savedAt = Date(timeIntervalSince1970: 1_700_000_000)
        try launchApp(articles: [
            ArticleFixture(id: photo, url: "https://example.com/photo", title: "Garden photograph",
                           savedAt: savedAt, kind: "image", imageType: "photograph", tags: ["Gardens"]),
            ArticleFixture(id: graphic, url: "https://example.com/graphic", title: "Garden diagram",
                           savedAt: savedAt, kind: "image", imageType: "other", tags: ["Gardens"]),
            ArticleFixture(id: unknown, url: "https://example.com/unknown", title: "Unclassified image",
                           savedAt: savedAt, kind: "image", tags: ["Gardens"]),
            ArticleFixture(id: article, url: "https://example.com/article", title: "Garden article",
                           savedAt: savedAt, tags: ["Gardens"])
        ]) { $0.pinnedDefaults = ["showsReadingInspector": "1"] }

        XCTAssertTrue(list.waitForRowCount(4))
        XCTAssertFalse(app.byId(A11y.Filter.imageType).exists)
        list.selectScope("Images")
        XCTAssertTrue(list.waitForRowCount(3))
        XCTAssertTrue(wait { self.imageTypeExists("Unsure") })
        list.selectImageType("Photos")
        XCTAssertTrue(wait { list.orderedRowIds == [photo] })
        list.selectImageType("Graphics")
        XCTAssertTrue(wait { list.orderedRowIds == [graphic] })

        list.selectImageType("Unsure")
        XCTAssertTrue(wait { list.orderedRowIds == [unknown] })
        list.open(unknown)
        let type = app.byId(A11y.Inspector.imageType)
        XCTAssertTrue(type.waitExists())
        type.clickWhenReady()
        XCTAssertTrue(wait {
            let contents = self.library.articleContents(id: unknown) ?? ""
            return contents.contains("image_type: photograph") && contents.contains("origin: user")
        }, "Correction is stored in the reading file")
        XCTAssertEqual(library.frontmatter(id: unknown)?.tags, ["Gardens"], "Type corrections preserve subject Tags")
        app.byId(A11y.Detail.close).clickWhenReady()
        XCTAssertTrue(wait {
            Set(list.orderedRowIds) == [photo, graphic, unknown] && !self.imageTypeExists("Unsure")
        }, "Classifying the last unsure image removes the segment and returns to All")
        list.selectImageType("Graphics")
        XCTAssertTrue(wait { list.orderedRowIds == [graphic] }, "Graphics contains only confirmed graphics")
        list.selectImageType("Photos")
        XCTAssertTrue(wait { Set(list.orderedRowIds) == [photo, unknown] })

        list.selectScope("Articles")
        XCTAssertTrue(wait { list.orderedRowIds == [article] })
        XCTAssertFalse(app.byId(A11y.Filter.imageType).exists)
        list.selectScope("Images")
        XCTAssertTrue(wait { Set(list.orderedRowIds) == [photo, unknown] }, "Images remembers its facet")

        relaunchApp()
        XCTAssertTrue(wait { Set(list.orderedRowIds) == [photo, unknown] }, "Scope and facet survive relaunch")
        list.open(unknown)
        app.byId(A11y.Inspector.automaticImageType).clickWhenReady()
        XCTAssertTrue(wait {
            !(self.library.articleContents(id: unknown) ?? "").contains("image_classification:")
        }, "Automatic reset clears the stored correction")
        app.byId(A11y.Detail.close).clickWhenReady()
        XCTAssertTrue(wait { list.orderedRowIds == [photo] }, "Reset leaves Photos immediately")
        XCTAssertTrue(wait { self.imageTypeExists("Unsure") }, "Automatic reset makes Unsure available again")
        list.selectImageType("Unsure")
        XCTAssertTrue(wait { list.orderedRowIds == [unknown] })
        list.selectImageType("Graphics")
        XCTAssertTrue(wait { list.orderedRowIds == [graphic] })
        list.selectImageType("All")
        XCTAssertTrue(wait { Set(list.orderedRowIds) == [photo, graphic, unknown] })
    }

    func testClassifiedLibraryHidesUnsureAndRestoresAllFromRememberedUnsure() throws {
        let photo = TestULID.make(705)
        let graphic = TestULID.make(706)
        let savedAt = Date(timeIntervalSince1970: 1_700_000_000)
        try launchApp(articles: [
            ArticleFixture(id: photo, url: "https://example.com/classified-photo", title: "Classified photo",
                           savedAt: savedAt, kind: "image", imageType: "photograph"),
            ArticleFixture(id: graphic, url: "https://example.com/classified-graphic", title: "Classified graphic",
                           savedAt: savedAt, kind: "image", imageType: "other")
        ]) { $0.pinnedDefaults = ["activeImageTypeFilter": "unsure"] }

        list.selectScope("Images")
        XCTAssertTrue(wait {
            Set(list.orderedRowIds) == [photo, graphic] && !self.imageTypeExists("Unsure")
        }, "A completed library shows All and omits Unsure, including after restoring its old preference")
    }

    private func imageTypeExists(_ label: String) -> Bool {
        let group = app.byId(A11y.Filter.imageType)
        return group.radioButtons[label].exists || group.buttons[label].exists
    }
}
