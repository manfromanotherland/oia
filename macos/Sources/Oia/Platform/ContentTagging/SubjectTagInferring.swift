// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// The operating system's readiness to infer subject labels on this Mac.
enum SubjectTaggingAvailability: Equatable, Sendable {
    case available
    case unavailable(Reason)

    enum Reason: Equatable, Sendable {
        case unsupportedOS
        case deviceNotEligible
        case appleIntelligenceNotEnabled
        case modelNotReady
    }
}

enum SubjectTaggingError: Error, Equatable, Sendable {
    case unavailable(SubjectTaggingAvailability.Reason)
}

/// Platform inference only. The Rust core owns tag merging and persistence.
protocol SubjectTagInferring: Sendable {
    /// Changes when the prompt or system model may produce different labels.
    var analyzerVersion: String { get }
    var availability: SubjectTaggingAvailability { get }

    /// Returns unmodified subject labels suggested by the on-device model.
    func inferSubjectLabels(from text: String) async throws -> [String]
}
