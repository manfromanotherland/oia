// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class ImageTypeFilterTests: XCTestCase {
    func testHidePhotographyKeepsImagesWithoutAPhotographyClassification() {
        let photo = makeReadingRow(kind: .image, imageType: .photograph)
        let graphic = makeReadingRow(kind: .image, imageType: .other)
        let unknown = makeReadingRow(kind: .image)

        XCTAssertTrue(ImageTypeFilter.all.contains(unknown))
        XCTAssertTrue(ImageTypeFilter.photography.contains(photo))
        XCTAssertFalse(ImageTypeFilter.photography.contains(graphic))
        XCTAssertFalse(ImageTypeFilter.photography.contains(unknown))
        XCTAssertTrue(ImageTypeFilter.hidePhotography.contains(graphic))
        XCTAssertTrue(ImageTypeFilter.hidePhotography.contains(unknown))
        XCTAssertFalse(ImageTypeFilter.hidePhotography.contains(photo))
        XCTAssertFalse(ImageTypeFilter.all.contains(makeReadingRow(kind: .video)))
        XCTAssertEqual(ImageTypeFilter.allCases.map(\.label), ["All images", "Hide photography", "Photography"])
    }

    func testExistingFilterPreferencesRestoreToTheNewModes() {
        XCTAssertEqual(ImageTypeFilter(savedValue: "photographs"), .photography)
        XCTAssertEqual(ImageTypeFilter(savedValue: "other"), .hidePhotography)
        XCTAssertEqual(ImageTypeFilter(savedValue: "unclassified"), .all)
        XCTAssertEqual(ImageTypeFilter(savedValue: "hidePhotography"), .hidePhotography)
        XCTAssertEqual(ImageTypeFilter(savedValue: "photography"), .photography)
        XCTAssertEqual(ImageTypeFilter(savedValue: nil), .all)
    }

    func testImageFacetComposesWithSearchAndPreservesBrowsingOrder() {
        for filter in ImageTypeFilter.allCases {
            let query = ReadingQuery.boardSnapshot(
                scope: .images, search: "garden", tagTerms: ["Interiors"],
                visualTerms: ["green"], colorTerms: ["#123456"], itemTypeTerms: ["image"],
                imageTypeFilter: filter,
                semanticCandidateIDs: ["text"], visualSemanticCandidateIDs: ["visual"]
            )
            XCTAssertEqual(query.kind, .image)
            XCTAssertEqual(query.itemTypeTerms, ["image"] + [filter.itemTypeTerm].compactMap { $0 })
            XCTAssertEqual(query.search, "garden")
            XCTAssertEqual(query.tagTerms, ["Interiors"])
            XCTAssertEqual(query.visualTerms, ["green"])
            XCTAssertEqual(query.colorTerms, ["#123456"])
            XCTAssertEqual(query.semanticCandidateIDs, ["text"])
            XCTAssertEqual(query.visualSemanticCandidateIDs, ["visual"])
        }

        let browsing = ReadingQuery.boardSnapshot(
            scope: .images, search: nil, tagTerms: [], visualTerms: [], imageTypeFilter: .photography,
            semanticCandidateIDs: [], visualSemanticCandidateIDs: []
        )
        guard case .savedAt = browsing.sort else {
            return XCTFail("An image facet keeps the newest-first browsing order")
        }
    }

    func testRememberedImageFacetDoesNotLeakIntoAnotherScope() {
        for scope in LibraryScope.allCases where scope != .images {
            let query = ReadingQuery.boardSnapshot(
                scope: scope, search: nil, tagTerms: [], visualTerms: [],
                imageTypeFilter: .photography, semanticCandidateIDs: [], visualSemanticCandidateIDs: []
            )
            XCTAssertTrue(query.itemTypeTerms.isEmpty, "\(scope) must ignore the remembered image facet")
        }
    }

    func testNewerCorrectionSurvivesOlderWriteCompletionAndRefresh() {
        let original = makeReadingRow(kind: .image, imageType: .photograph)
        var pending = PendingImageTypeEdits()
        let first = pending.reserve(ids: [original.id], imageType: .other, librarySession: 1)
        let second = pending.reserve(ids: [original.id], imageType: .photograph, librarySession: 1)
        pending.complete(first)
        var olderStoredRow = original
        olderStoredRow.imageType = .other
        XCTAssertEqual(pending.apply(to: olderStoredRow, librarySession: 1).imageType, .photograph)
        XCTAssertEqual(pending.apply(to: olderStoredRow, librarySession: 2).imageType, .other)
        pending.complete(second)
        XCTAssertEqual(pending.apply(to: olderStoredRow, librarySession: 1).imageType, .other,
                       "A settled or failed write returns to file-backed truth")
    }

    func testAutomaticResetImmediatelyClearsTheTypeAndLeavesPhotographs() {
        let photo = makeReadingRow(kind: .image, imageType: .photograph)
        var pending = PendingImageTypeEdits()
        let reset = pending.reserve(ids: [photo.id], imageType: nil, librarySession: 1)
        let resetRow = pending.apply(to: photo, librarySession: 1)
        XCTAssertNil(resetRow.imageType)
        XCTAssertFalse(ImageTypeFilter.photography.contains(resetRow))
        XCTAssertTrue(ImageTypeFilter.hidePhotography.contains(resetRow))
        pending.complete(reset)
        var resolved = photo
        resolved.imageType = nil
        XCTAssertNil(pending.apply(to: resolved, librarySession: 1).imageType)
    }
}
