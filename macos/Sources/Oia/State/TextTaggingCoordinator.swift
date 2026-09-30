// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Serial system-model inference over Rust-owned snapshots and file mutations.
actor TextTaggingCoordinator {
    private static let modelReadinessRetryLimit = 6
    private static let modelReadinessRetryDelay: Duration = .seconds(10)

    private let tagger: any SubjectTagInferring
    private let sleepForModelReadiness: @Sendable (Duration) async throws -> Void

    init(
        tagger: any SubjectTagInferring,
        sleepForModelReadiness: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.tagger = tagger
        self.sleepForModelReadiness = sleepForModelReadiness
    }

    func reconcile(
        core: any TextTaggingCore,
        didApplyBatch: @Sendable (Int) async -> Void
    ) async throws {
        var readinessRetriesRemaining = Self.modelReadinessRetryLimit
        guard try await waitForModelReadiness(retriesRemaining: &readinessRetriesRemaining) else {
            return
        }
        let version = tagger.analyzerVersion
        var cursor: String?
        repeat {
            try Task.checkCancellation()
            let batch = try await core.pendingTextTagging(
                analyzerVersion: version, limit: 8, afterReadingID: cursor
            )
            var appliedCount = 0
            for task in batch.tasks {
                try Task.checkCancellation()
                var retriedAfterModelNotReady = false
                while true {
                    try Task.checkCancellation()
                    do {
                        let tags = try await tagger.inferSubjectLabels(from: task.text)
                        try Task.checkCancellation()
                        if try await core.completeTextTagging(task: task, tags: tags) {
                            appliedCount += 1
                        }
                        break
                    } catch is CancellationError {
                        throw CancellationError()
                    } catch SubjectTaggingError.unavailable(.modelNotReady) {
                        guard !retriedAfterModelNotReady,
                              try await waitForModelReadiness(
                                  retriesRemaining: &readinessRetriesRemaining
                              ) else {
                            if appliedCount > 0 { await didApplyBatch(appliedCount) }
                            return
                        }
                        retriedAfterModelNotReady = true
                    } catch is SubjectTaggingError {
                        // Disabled or ineligible models cannot become ready
                        // during this pass. Leave the snapshot pending.
                        if appliedCount > 0 { await didApplyBatch(appliedCount) }
                        return
                    } catch {
                        // One refusal or damaged reading must not prevent the
                        // rest of the library from being tagged.
                        break
                    }
                }
            }
            if appliedCount > 0 { await didApplyBatch(appliedCount) }
            cursor = batch.nextReadingID
        } while cursor != nil
    }

    private func waitForModelReadiness(retriesRemaining: inout Int) async throws -> Bool {
        while true {
            try Task.checkCancellation()
            switch tagger.availability {
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
