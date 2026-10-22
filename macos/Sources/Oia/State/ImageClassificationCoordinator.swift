// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Runs expensive pixel inference serially, outside the UI. Paging and durable
/// decisions remain behind the core's interface.
actor ImageClassificationCoordinator {
    private let classifier: any ImageTypeClassifying

    init(classifier: any ImageTypeClassifying) {
        self.classifier = classifier
    }

    func reconcile(
        core: any ImageClassificationCore,
        didApplyBatch: @Sendable (Int) async -> Void
    ) async throws {
        guard classifier.isAvailable else { return }
        let version = classifier.analyzerVersion
        var cursor: String?
        repeat {
            try Task.checkCancellation()
            let batch = try await core.pendingImageClassification(
                analyzerVersion: version, limit: 4, afterReadingID: cursor
            )
            var changed = 0
            for asset in batch.assets {
                try Task.checkCancellation()
                guard classifier.isAvailable else {
                    if changed > 0 { await didApplyBatch(changed) }
                    return
                }
                do {
                    let assessment = try await classifier.classify(imageAt: asset.fileURL)
                    try Task.checkCancellation()
                    if try await core.completeImageClassification(
                        asset: asset, analyzerVersion: version, assessment: assessment
                    ) {
                        changed += 1
                    }
                } catch is CancellationError {
                    throw CancellationError()
                } catch {
                    // A failed model request leaves this image pending; later
                    // images must still get a chance during the current pass.
                }
            }
            if changed > 0 { await didApplyBatch(changed) }
            cursor = batch.nextReadingID
        } while cursor != nil
    }
}
