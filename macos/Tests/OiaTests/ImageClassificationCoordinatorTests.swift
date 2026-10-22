// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import XCTest

final class ImageClassificationCoordinatorTests: XCTestCase {
    func testUnavailableModelLeavesEveryImagePending() async throws {
        let core = ClassificationCoreFixture()
        let coordinator = ImageClassificationCoordinator(classifier: ClassificationFixture(available: false))
        try await coordinator.reconcile(core: core) { _ in XCTFail("Unavailable model published a result") }
        let requests = await core.requests
        XCTAssertEqual(requests, 0)
    }

    func testFailedImageDoesNotStarveLaterPagesAndUnknownIsRecorded() async throws {
        let core = ClassificationCoreFixture()
        let updates = ClassificationUpdates()
        let coordinator = ImageClassificationCoordinator(classifier: ClassificationFixture())
        try await coordinator.reconcile(core: core) { count in await updates.record(count) }
        let completed = await core.completed
        let published = await updates.values
        XCTAssertEqual(completed, ["uncertain", "photo"])
        let assessments = await core.assessments
        XCTAssertEqual(assessments, [.unknown, .photograph])
        XCTAssertEqual(published, [1, 1])
    }

    func testStaleCompletionDoesNotPublish() async throws {
        let core = ClassificationCoreFixture(accepts: false)
        let coordinator = ImageClassificationCoordinator(classifier: ClassificationFixture())
        try await coordinator.reconcile(core: core) { _ in XCTFail("Stale analysis published") }
    }

    func testCancellationAfterInferenceDoesNotPersist() async throws {
        let core = ClassificationCoreFixture()
        let coordinator = ImageClassificationCoordinator(classifier: ClassificationFixture(cancels: true))
        do {
            try await coordinator.reconcile(core: core) { _ in XCTFail("Cancelled analysis published") }
            XCTFail("Cancellation should propagate")
        } catch is CancellationError {}
        let completed = await core.completed
        XCTAssertTrue(completed.isEmpty)
    }
}

private struct ClassificationFixture: ImageTypeClassifying {
    var available = true
    var cancels = false
    var analyzerVersion: String { "test-medium-v1" }
    var isAvailable: Bool { available }

    func classify(imageAt url: URL) async throws -> ImageTypeAssessment {
        if cancels { throw CancellationError() }
        switch url.lastPathComponent {
        case "broken": throw CocoaError(.fileReadCorruptFile)
        case "uncertain": return .unknown
        default: return .photograph
        }
    }
}

private actor ClassificationCoreFixture: ImageClassificationCore {
    var requests = 0
    var completed: [String] = []
    var assessments: [ImageTypeAssessment] = []
    let accepts: Bool

    init(accepts: Bool = true) { self.accepts = accepts }

    func pendingImageClassification(
        analyzerVersion _: String, limit _: UInt32, afterReadingID: String?
    ) async throws -> ImageClassificationBatch {
        requests += 1
        let ids = afterReadingID == nil ? ["broken", "uncertain"] : ["photo"]
        return ImageClassificationBatch(
            assets: ids.map { id in
                VisualAssetSnapshot(
                    readingID: id, title: id, relativePath: "assets/\(id)",
                    fileURL: URL(fileURLWithPath: "/tmp/\(id)"), contentHash: id
                )
            },
            nextReadingID: afterReadingID == nil ? "uncertain" : nil
        )
    }

    func completeImageClassification(
        asset: VisualAssetSnapshot, analyzerVersion _: String, assessment: ImageTypeAssessment
    ) async throws -> Bool {
        completed.append(asset.readingID)
        assessments.append(assessment)
        return accepts
    }
}

private actor ClassificationUpdates {
    var values: [Int] = []
    func record(_ count: Int) { values.append(count) }
}
