// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A structured model assessment. Unknown is a completed, uncertain result;
/// an unavailable model or failed inference leaves the source pending instead.
enum ImageTypeAssessment: String, Sendable {
    case photograph
    case other
    case unknown
}

enum ImageClassificationAvailability: Equatable, Sendable {
    case available
    case unavailable(Reason)

    enum Reason: Equatable, Sendable {
        case unsupportedOS
        case deviceNotEligible
        case appleIntelligenceNotEnabled
        case modelNotReady
    }
}

enum ImageClassificationError: Error, Equatable, Sendable {
    case unavailable(ImageClassificationAvailability.Reason)
}

protocol ImageTypeClassifying: Sendable {
    var analyzerVersion: String { get }
    var availability: ImageClassificationAvailability { get }
    func classify(imageAt url: URL) async throws -> ImageTypeAssessment
}
