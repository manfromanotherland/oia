// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A structured model assessment. Unknown is a completed, uncertain result;
/// an unavailable model or failed inference leaves the source pending instead.
enum ImageTypeAssessment: String, Sendable {
    case photograph
    case other
    case unknown
}

protocol ImageTypeClassifying: Sendable {
    var analyzerVersion: String { get }
    var isAvailable: Bool { get }
    func classify(imageAt url: URL) async throws -> ImageTypeAssessment
}
