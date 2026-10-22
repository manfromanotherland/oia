// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

struct ImageClassificationBatch: Sendable {
    let assets: [VisualAssetSnapshot]
    let nextReadingID: String?
}

/// Rust supplies immutable image snapshots and owns classification provenance,
/// source validation, user precedence, and file-first persistence.
protocol ImageClassificationCore: Sendable {
    func pendingImageClassification(
        analyzerVersion: String, limit: UInt32, afterReadingID: String?
    ) async throws -> ImageClassificationBatch
    @discardableResult func completeImageClassification(
        asset: VisualAssetSnapshot, analyzerVersion: String, assessment: ImageTypeAssessment
    ) async throws -> Bool
}
