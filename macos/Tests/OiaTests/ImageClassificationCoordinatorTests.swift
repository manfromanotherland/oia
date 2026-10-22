// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Synchronization
import XCTest

final class ImageClassificationCoordinatorTests: XCTestCase {
    func testUnavailableModelLeavesEveryImagePending() async throws {
        for reason: ImageClassificationAvailability.Reason in [
            .unsupportedOS, .deviceNotEligible, .appleIntelligenceNotEnabled
        ] {
            let core = ClassificationCoreFixture()
            let delays = ClassificationRetryDelays()
            let coordinator = ImageClassificationCoordinator(
                classifier: ClassificationFixture(availability: .unavailable(reason))
            ) { delay in
                await delays.record(delay)
            }
            try await coordinator.reconcile(core: core) { _ in XCTFail("Unavailable model published a result") }
            let requests = await core.requests
            let recordedDelays = await delays.values
            XCTAssertEqual(requests, 0)
            XCTAssertTrue(recordedDelays.isEmpty, "Disabled or unsupported models must not poll")
        }
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

    func testTemporaryModelInterruptionStillClassifiesLaterPages() async throws {
        let core = ClassificationCoreFixture()
        let classifier = InterruptedClassificationFixture()
        let delays = ClassificationRetryDelays()
        let coordinator = ImageClassificationCoordinator(classifier: classifier) { delay in
            await delays.record(delay)
        }

        try await coordinator.reconcile(core: core) { _ in }

        let completed = await core.completed
        XCTAssertEqual(completed, ["uncertain", "photo"], "A temporary model interruption must not strand later images")
        let recordedDelays = await delays.values
        XCTAssertEqual(recordedDelays, [.seconds(10)])
    }

    func testModelReadinessErrorRetriesTheCurrentImageAfterRecovery() async throws {
        let core = ClassificationCoreFixture()
        let coordinator = ImageClassificationCoordinator(classifier: ReadinessErrorClassificationFixture())

        try await coordinator.reconcile(core: core) { _ in }

        let completed = await core.completed
        XCTAssertEqual(completed, ["uncertain", "photo"], "A readiness change during inference must retry the current image")
    }

    func testModelNotReadyStopsAfterBoundedRetriesWithoutQueryingImages() async throws {
        let core = ClassificationCoreFixture()
        let delays = ClassificationRetryDelays()
        let coordinator = ImageClassificationCoordinator(
            classifier: ClassificationFixture(availability: .unavailable(.modelNotReady))
        ) { delay in
            await delays.record(delay)
        }

        try await coordinator.reconcile(core: core) { _ in XCTFail("An unavailable model published a result") }

        let recordedDelays = await delays.values
        let requests = await core.requests
        XCTAssertEqual(recordedDelays, Array(repeating: .seconds(10), count: 6))
        XCTAssertEqual(requests, 0)
    }

    func testCancellationDuringModelReadinessWaitDoesNotQueryImages() async throws {
        let core = ClassificationCoreFixture()
        let coordinator = ImageClassificationCoordinator(
            classifier: ClassificationFixture(availability: .unavailable(.modelNotReady))
        ) { _ in throw CancellationError() }

        do {
            try await coordinator.reconcile(core: core) { _ in XCTFail("Cancelled analysis published a result") }
            XCTFail("Readiness cancellation must propagate")
        } catch is CancellationError {}

        let requests = await core.requests
        XCTAssertEqual(requests, 0)
    }
}

private final class ReadinessErrorClassificationFixture: ImageTypeClassifying {
    let analyzerVersion = "test-medium-v1"
    let availability: ImageClassificationAvailability = .available
    private let hasInterrupted = Mutex(false)

    func classify(imageAt url: URL) async throws -> ImageTypeAssessment {
        switch url.lastPathComponent {
        case "broken": throw CocoaError(.fileReadCorruptFile)
        case "uncertain":
            let shouldInterrupt = hasInterrupted.withLock { interrupted in
                defer { interrupted = true }
                return !interrupted
            }
            if shouldInterrupt { throw ImageClassificationError.unavailable(.modelNotReady) }
            return .unknown
        default: return .photograph
        }
    }
}

private final class InterruptedClassificationFixture: ImageTypeClassifying {
    let analyzerVersion = "test-medium-v1"
    private let checks = Mutex(0)

    var availability: ImageClassificationAvailability {
        checks.withLock { count in
            count += 1
            return count == 4 ? .unavailable(.modelNotReady) : .available
        }
    }

    func classify(imageAt url: URL) async throws -> ImageTypeAssessment {
        switch url.lastPathComponent {
        case "broken": throw CocoaError(.fileReadCorruptFile)
        case "uncertain": return .unknown
        default: return .photograph
        }
    }
}

private struct ClassificationFixture: ImageTypeClassifying {
    var availability: ImageClassificationAvailability = .available
    var cancels = false
    var analyzerVersion: String { "test-medium-v1" }

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

private actor ClassificationRetryDelays {
    var values: [Duration] = []
    func record(_ duration: Duration) { values.append(duration) }
}
