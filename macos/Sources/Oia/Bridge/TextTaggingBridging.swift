// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A bounded text snapshot whose fingerprint the core revalidates on completion.
struct TextTaggingWorkItem: Equatable, Sendable {
    let readingID: String
    let title: String
    let text: String
    let sourceFingerprint: String
    let analyzerVersion: String

    init(
        readingID: String,
        title: String,
        text: String,
        sourceFingerprint: String,
        analyzerVersion: String
    ) {
        self.readingID = readingID
        self.title = title
        self.text = text
        self.sourceFingerprint = sourceFingerprint
        self.analyzerVersion = analyzerVersion
    }

    init(_ task: FfiTextTaggingTask) {
        readingID = task.readingId
        title = task.title
        text = task.text
        sourceFingerprint = task.sourceFingerprint
        analyzerVersion = task.analyzerVersion
    }

    var ffi: FfiTextTaggingTask {
        FfiTextTaggingTask(
            readingId: readingID, title: title, text: text,
            sourceFingerprint: sourceFingerprint, analyzerVersion: analyzerVersion
        )
    }
}

struct TextTaggingBatch: Sendable {
    let tasks: [TextTaggingWorkItem]
    let nextReadingID: String?

    init(tasks: [TextTaggingWorkItem], nextReadingID: String?) {
        self.tasks = tasks
        self.nextReadingID = nextReadingID
    }

    init(_ batch: FfiTextTaggingBatch) {
        tasks = batch.tasks.map(TextTaggingWorkItem.init)
        nextReadingID = batch.nextReadingId
    }
}

protocol TextTaggingCore: Sendable {
    func pendingTextTagging(
        analyzerVersion: String, limit: UInt32, afterReadingID: String?
    ) async throws -> TextTaggingBatch
    @discardableResult func completeTextTagging(
        task: TextTaggingWorkItem, tags: [String]
    ) async throws -> Bool
}
