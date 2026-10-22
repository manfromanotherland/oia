// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Platform enrichments share a library session but have independent results.
@MainActor
enum VisualSearchReconciliationPass {
    static func run(
        visualSearch: @escaping @MainActor @Sendable () async throws -> Void,
        textTagging: @escaping @MainActor @Sendable () async throws -> Void,
        imageClassification: @escaping @MainActor @Sendable () async throws -> Void
    ) async {
        async let visual: Void = perform(visualSearch)
        async let text: Void = perform(textTagging)
        async let images: Void = perform(imageClassification)
        _ = await (visual, text, images)
    }

    private static func perform(_ work: @MainActor @Sendable () async throws -> Void) async {
        do {
            try Task.checkCancellation()
            try await work()
        } catch {
            // Each enrichment is optional. A failure in one must not cancel
            // another's remaining images; parent cancellation still propagates.
        }
    }
}
