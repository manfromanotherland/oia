// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Runs expensive pixel inference serially, outside the UI. Paging and durable
/// decisions remain behind the core's interface.
actor ImageClassificationCoordinator {
    private static let modelReadinessRetryLimit = 6
    private static let modelReadinessRetryDelay: Duration = .seconds(10)

    private let classifier: any ImageTypeClassifying
    private let sleepForModelReadiness: @Sendable (Duration) async throws -> Void

    init(
        classifier: any ImageTypeClassifying,
        sleepForModelReadiness: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.classifier = classifier
        self.sleepForModelReadiness = sleepForModelReadiness
    }

    func reconcile(
        core: any ImageClassificationCore,
        didApplyBatch: @Sendable (Int) async -> Void
    ) async throws {
        var readinessRetriesRemaining = Self.modelReadinessRetryLimit
        guard try await waitForModelReadiness(retriesRemaining: &readinessRetriesRemaining) else {
            return
        }
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
                guard try await waitForModelReadiness(retriesRemaining: &readinessRetriesRemaining) else {
                    if changed > 0 { await didApplyBatch(changed) }
                    return
                }
                var retriedAfterModelNotReady = false
                while true {
                    try Task.checkCancellation()
                    do {
                        let assessment = try await classifier.classify(imageAt: asset.fileURL)
                        try Task.checkCancellation()
                        if try await core.completeImageClassification(
                            asset: asset, analyzerVersion: version, assessment: assessment
                        ) {
                            changed += 1
                        }
                        break
                    } catch is CancellationError {
                        throw CancellationError()
                    } catch ImageClassificationError.unavailable(.modelNotReady) {
                        guard !retriedAfterModelNotReady,
                              try await waitForModelReadiness(
                                  retriesRemaining: &readinessRetriesRemaining
                              ) else {
                            if changed > 0 { await didApplyBatch(changed) }
                            return
                        }
                        retriedAfterModelNotReady = true
                    } catch is ImageClassificationError {
                        // Disabled and ineligible models cannot recover during
                        // this pass. Preserve the pending snapshot for later.
                        if changed > 0 { await didApplyBatch(changed) }
                        return
                    } catch {
                        // A failed model request leaves this image pending;
                        // later images still get a chance in the current pass.
                        break
                    }
                }
            }
            if changed > 0 { await didApplyBatch(changed) }
            cursor = batch.nextReadingID
        } while cursor != nil
    }

    private func waitForModelReadiness(retriesRemaining: inout Int) async throws -> Bool {
        while true {
            try Task.checkCancellation()
            switch classifier.availability {
            case .available:
                return true
            case .unavailable(.modelNotReady):
                guard retriesRemaining > 0 else { return false }
                retriesRemaining -= 1
                try await sleepForModelReadiness(Self.modelReadinessRetryDelay)
            case .unavailable:
                return false
            }
        }
    }
}
