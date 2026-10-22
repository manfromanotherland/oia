// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class ImageTypeFilterTests: XCTestCase {
    func testGraphicsRequiresAConfirmedGraphicClassification() {
        let photo = makeReadingRow(kind: .image, imageType: .photograph)
        let graphic = makeReadingRow(kind: .image, imageType: .other)
        let unknown = makeReadingRow(kind: .image)

        XCTAssertTrue(ImageTypeFilter.all.contains(unknown))
        XCTAssertTrue(ImageTypeFilter.photos.contains(photo))
        XCTAssertFalse(ImageTypeFilter.photos.contains(graphic))
        XCTAssertFalse(ImageTypeFilter.photos.contains(unknown))
        XCTAssertTrue(ImageTypeFilter.graphics.contains(graphic))
        XCTAssertFalse(ImageTypeFilter.graphics.contains(unknown))
        XCTAssertFalse(ImageTypeFilter.graphics.contains(photo))
        XCTAssertTrue(ImageTypeFilter.unsure.contains(unknown))
        XCTAssertFalse(ImageTypeFilter.unsure.contains(photo))
        XCTAssertFalse(ImageTypeFilter.unsure.contains(graphic))
        XCTAssertFalse(ImageTypeFilter.unsure.contains(makeReadingRow(kind: .video)))
        XCTAssertFalse(ImageTypeFilter.all.contains(makeReadingRow(kind: .video)))
        XCTAssertEqual(ImageTypeFilter.allCases.map(\.label), ["All", "Graphics", "Photos", "Unsure"])
        XCTAssertEqual(ImageTypeFilter.graphics.itemTypeTerm, "other_image")
        XCTAssertEqual(ImageTypeFilter.unsure.itemTypeTerm, "unclassified_image")
    }

    func testExistingFilterPreferencesRestoreToTheNewModes() {
        XCTAssertEqual(ImageTypeFilter(savedValue: "photographs"), .photos)
        XCTAssertEqual(ImageTypeFilter(savedValue: "other"), .graphics)
        XCTAssertEqual(ImageTypeFilter(savedValue: "unclassified"), .unsure)
        XCTAssertEqual(ImageTypeFilter(savedValue: "unsure"), .unsure)
        XCTAssertEqual(ImageTypeFilter(savedValue: "hidePhotography"), .graphics)
        XCTAssertEqual(ImageTypeFilter(savedValue: "photography"), .photos)
        XCTAssertEqual(ImageTypeFilter(savedValue: "graphics"), .graphics)
        XCTAssertEqual(ImageTypeFilter(savedValue: "photos"), .photos)
        XCTAssertEqual(ImageTypeFilter(savedValue: nil), .all)
    }

    func testUnsureIsAvailableOnlyWhileImagesNeedClassification() {
        XCTAssertEqual(
            ImageTypeFilter.availableCases(hasUnsureImages: false),
            [.all, .graphics, .photos]
        )
        XCTAssertEqual(
            ImageTypeFilter.availableCases(hasUnsureImages: true),
            [.all, .graphics, .photos, .unsure]
        )
    }

    func testOnlyAnUnavailableUnsureSelectionFallsBackToAll() {
        for filter in ImageTypeFilter.allCases {
            XCTAssertEqual(filter.availableSelection(hasUnsureImages: true), filter)
            XCTAssertEqual(
                filter.availableSelection(hasUnsureImages: false),
                filter == .unsure ? .all : filter
            )
        }
    }

    func testUnsureAvailabilityQueriesTheWholeLibraryWithOneResult() {
        let query = ReadingQuery.unsureAvailability()

        XCTAssertEqual(query.scope, .images)
        XCTAssertEqual(query.kind, .image)
        XCTAssertEqual(query.itemTypeTerms, ["unclassified_image"])
        XCTAssertNil(query.tag)
        XCTAssertNil(query.search)
        XCTAssertTrue(query.tagTerms.isEmpty)
        XCTAssertTrue(query.visualTerms.isEmpty)
        XCTAssertTrue(query.colorTerms.isEmpty)
        XCTAssertTrue(query.semanticCandidateIDs.isEmpty)
        XCTAssertTrue(query.visualSemanticCandidateIDs.isEmpty)
        XCTAssertEqual(query.limit, 1)
        XCTAssertEqual(query.offset, 0)
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
            scope: .images, search: nil, tagTerms: [], visualTerms: [], imageTypeFilter: .photos,
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
                imageTypeFilter: .photos, semanticCandidateIDs: [], visualSemanticCandidateIDs: []
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
        XCTAssertFalse(ImageTypeFilter.photos.contains(resetRow))
        XCTAssertFalse(ImageTypeFilter.graphics.contains(resetRow))
        XCTAssertTrue(ImageTypeFilter.unsure.contains(resetRow))
        pending.complete(reset)
        var resolved = photo
        resolved.imageType = nil
        XCTAssertNil(pending.apply(to: resolved, librarySession: 1).imageType)
    }
}
