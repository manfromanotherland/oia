// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import FoundationModels

/// Apple's on-device content-tagging model, used only for text inference.
struct AppleSubjectTagger: SubjectTagInferring {
    private static let promptRevision = "topics-v2"

    /// Apple updates its system model with OS releases. Revisit generated tags
    /// after an update without relying on a model identifier unavailable on 26.
    var analyzerVersion: String {
        let version = ProcessInfo.processInfo.operatingSystemVersion
        return "apple-foundation-content-tagging-\(Self.promptRevision)-macos-\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)"
    }

    var availability: SubjectTaggingAvailability {
        guard #available(macOS 26.0, *) else {
            return .unavailable(.unsupportedOS)
        }
        return Self.availability(of: SystemLanguageModel(useCase: .contentTagging))
    }

    func inferSubjectLabels(from text: String) async throws -> [String] {
        guard #available(macOS 26.0, *) else {
            throw SubjectTaggingError.unavailable(.unsupportedOS)
        }

        let model = SystemLanguageModel(useCase: .contentTagging)
        if case let .unavailable(reason) = Self.availability(of: model) {
            throw SubjectTaggingError.unavailable(reason)
        }

        let input = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !input.isEmpty else { return [] }

        // A new session per reading prevents labels from previous readings from
        // entering the model's context. The core supplies a bounded text excerpt.
        let session = LanguageModelSession(model: model, instructions: """
            Provide 2 to 5 short subject-topic tags actually supported by the input text.
            Keep each tag at most 20 characters long.
            Avoid generic format labels such as article, quote, tweet, or image.
            """)
        let response = try await session.respond(
            to: input,
            generating: SubjectTopics.self,
            options: GenerationOptions(samplingMode: .greedy)
        )
        return response.content.topics
    }

    @available(macOS 26.0, *)
    private static func availability(of model: SystemLanguageModel) -> SubjectTaggingAvailability {
        switch model.availability {
        case .available:
            return .available
        case .unavailable(.deviceNotEligible):
            return .unavailable(.deviceNotEligible)
        case .unavailable(.appleIntelligenceNotEnabled):
            return .unavailable(.appleIntelligenceNotEnabled)
        case .unavailable(.modelNotReady):
            return .unavailable(.modelNotReady)
        @unknown default:
            return .unavailable(.modelNotReady)
        }
    }
}

@available(macOS 26.0, *)
@Generable
private struct SubjectTopics {
    @Guide(description: "The main subject topics, each at most 20 characters.", .maximumCount(5))
    var topics: [String]
}
