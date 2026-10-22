// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import XCTest

@MainActor
final class VisualSearchReconciliationPassTests: XCTestCase {
    func testTextTaggingFailureDoesNotStopLaterImageClassification() async {
        let progress = ReconciliationPassProgress()

        await VisualSearchReconciliationPass.run(
            visualSearch: {},
            textTagging: {
                await progress.waitForFirstImage()
                throw ReconciliationPassFailure.platform
            },
            imageClassification: {
                await progress.recordImage()
                try await Task.sleep(for: .seconds(1))
                try Task.checkCancellation()
                await progress.recordImage()
            }
        )

        let classified = await progress.classified
        XCTAssertEqual(classified, 2, "Text tagging must not cancel classification after its first image")
    }

    func testVisualIndexFailureDoesNotStopLaterImageClassification() async {
        let progress = ReconciliationPassProgress()

        await VisualSearchReconciliationPass.run(
            visualSearch: {
                await progress.waitForFirstImage()
                throw ReconciliationPassFailure.platform
            },
            textTagging: {},
            imageClassification: {
                await progress.recordImage()
                try await Task.sleep(for: .seconds(1))
                try Task.checkCancellation()
                await progress.recordImage()
            }
        )

        let classified = await progress.classified
        XCTAssertEqual(classified, 2, "Visual indexing must not cancel classification after its first image")
    }

    func testReplacingTheLibraryCancelsEveryEnrichment() async {
        let cancellation = ReconciliationPassCancellation()
        let work: @MainActor @Sendable () async throws -> Void = {
            await cancellation.recordStart()
            do {
                try await Task.sleep(for: .seconds(60))
            } catch is CancellationError {
                await cancellation.recordCancellation()
                throw CancellationError()
            }
        }
        let pass = Task {
            await VisualSearchReconciliationPass.run(
                visualSearch: work,
                textTagging: work,
                imageClassification: work
            )
        }
        await cancellation.waitForEveryStart()
        pass.cancel()
        await pass.value

        let cancelled = await cancellation.cancelled
        XCTAssertEqual(cancelled, 3, "Session cancellation must still reach every independent enrichment")
    }
}

private actor ReconciliationPassCancellation {
    var cancelled = 0
    private var started = 0
    private var startWaiters: [CheckedContinuation<Void, Never>] = []

    func recordStart() {
        started += 1
        guard started == 3 else { return }
        let waiters = startWaiters
        startWaiters.removeAll()
        waiters.forEach { $0.resume() }
    }

    func recordCancellation() { cancelled += 1 }

    func waitForEveryStart() async {
        guard started < 3 else { return }
        await withCheckedContinuation { startWaiters.append($0) }
    }
}

private enum ReconciliationPassFailure: Error {
    case platform
}

private actor ReconciliationPassProgress {
    var classified = 0
    private var firstImageWaiters: [CheckedContinuation<Void, Never>] = []

    func recordImage() {
        classified += 1
        let waiters = firstImageWaiters
        firstImageWaiters.removeAll()
        waiters.forEach { $0.resume() }
    }

    func waitForFirstImage() async {
        guard classified == 0 else { return }
        await withCheckedContinuation { firstImageWaiters.append($0) }
    }
}
