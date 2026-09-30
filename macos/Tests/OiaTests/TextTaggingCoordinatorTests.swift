// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Synchronization
import XCTest

final class TextTaggingCoordinatorTests: XCTestCase {
    func testUnavailableModelSkipsCoreQuery() async throws {
        let core = FakeTextTaggingCore(tasks: [makeTask("a")])
        let tagger = FakeSubjectTagger(availability: .unavailable(.appleIntelligenceNotEnabled))
        let updates = AppliedBatchRecorder()

        try await TextTaggingCoordinator(tagger: tagger).reconcile(core: core) { count in
            await updates.record(count)
        }

        let pending = await core.pendingCalls()
        let inputs = await tagger.inputs()
        let published = await updates.counts()
        XCTAssertTrue(pending.isEmpty)
        XCTAssertTrue(inputs.isEmpty)
        XCTAssertTrue(published.isEmpty)
    }

    func testModelNotReadyRetriesThenProcessesWhenAvailable() async throws {
        let task = makeTask("a")
        let core = FakeTextTaggingCore(tasks: [task])
        let tagger = MutableReadinessTagger(availability: .unavailable(.modelNotReady))
        let retryClock = RetryDelayRecorder()
        let updates = AppliedBatchRecorder()
        let coordinator = TextTaggingCoordinator(tagger: tagger) { delay in
            await retryClock.record(delay)
            tagger.setAvailability(.available)
        }

        try await coordinator.reconcile(core: core) { count in
            await updates.record(count)
        }

        let delays = await retryClock.delays()
        let pending = await core.pendingCalls()
        let accepted = await core.acceptedCompletions()
        let published = await updates.counts()
        XCTAssertEqual(delays, [.seconds(10)])
        XCTAssertEqual(pending.count, 1)
        XCTAssertEqual(accepted, [.init(task: task, tags: ["architecture"])])
        XCTAssertEqual(published, [1])
    }

    func testModelNotReadyStopsAfterBoundedRetriesWithoutQueryingCore() async throws {
        let core = FakeTextTaggingCore(tasks: [makeTask("a")])
        let tagger = MutableReadinessTagger(availability: .unavailable(.modelNotReady))
        let retryClock = RetryDelayRecorder()
        let coordinator = TextTaggingCoordinator(tagger: tagger) { delay in
            await retryClock.record(delay)
        }

        try await coordinator.reconcile(core: core) { _ in }

        let delays = await retryClock.delays()
        let pending = await core.pendingCalls()
        XCTAssertEqual(delays, Array(repeating: .seconds(10), count: 6))
        XCTAssertTrue(pending.isEmpty)
    }

    func testCancellationDuringModelReadinessWaitPreventsCoreQuery() async throws {
        let core = FakeTextTaggingCore(tasks: [makeTask("a")])
        let tagger = MutableReadinessTagger(availability: .unavailable(.modelNotReady))
        let retryClock = SuspendedRetryClock()
        let coordinator = TextTaggingCoordinator(tagger: tagger) { delay in
            await retryClock.sleep(for: delay)
        }
        let reconciliation = Task {
            try await coordinator.reconcile(core: core) { _ in }
        }

        await retryClock.waitUntilSleeping()
        reconciliation.cancel()
        await retryClock.finishSleep()

        do {
            try await reconciliation.value
            XCTFail("Cancellation should stop model readiness polling")
        } catch is CancellationError {
            // Cancellation is observed before another availability check.
        }

        let pending = await core.pendingCalls()
        XCTAssertTrue(pending.isEmpty)
    }

    func testStaleCompletionDoesNotPublishAppliedTags() async throws {
        let task = makeTask("stale")
        let core = FakeTextTaggingCore(tasks: [task], staleReadingIDs: [task.readingID])
        let tagger = FakeSubjectTagger(outcomes: [task.text: .labels(["furniture design"])])
        let updates = AppliedBatchRecorder()

        try await TextTaggingCoordinator(tagger: tagger).reconcile(core: core) { count in
            await updates.record(count)
        }

        let attempts = await core.completionAttempts()
        XCTAssertEqual(attempts, [.init(task: task, tags: ["furniture design"])])
        let accepted = await core.acceptedCompletions()
        let published = await updates.counts()
        XCTAssertTrue(accepted.isEmpty)
        XCTAssertTrue(published.isEmpty)
    }

    func testInferenceFailureContinuesToFollowingItemsAndNextPage() async throws {
        let tasks = ["a", "b", "c"].map { makeTask($0) }
        let core = FakeTextTaggingCore(tasks: tasks, pageSize: 2)
        let tagger = FakeSubjectTagger(outcomes: [
            tasks[0].text: .failure,
            tasks[1].text: .labels(["industrial design"]),
            tasks[2].text: .labels(["architecture"])
        ])
        let updates = AppliedBatchRecorder()

        try await TextTaggingCoordinator(tagger: tagger).reconcile(core: core) { count in
            await updates.record(count)
        }

        let inputs = await tagger.inputs()
        let pending = await core.pendingCalls()
        let accepted = await core.acceptedCompletions()
        let published = await updates.counts()
        XCTAssertEqual(inputs, tasks.map(\.text))
        XCTAssertEqual(pending, [
            .init(analyzerVersion: "tagger-v1", limit: 8, afterReadingID: nil),
            .init(analyzerVersion: "tagger-v1", limit: 8, afterReadingID: "b")
        ])
        XCTAssertEqual(accepted.map(\.task.readingID), ["b", "c"])
        XCTAssertEqual(published, [1, 1])
    }

    func testCancellationBeforeInferenceReturnsPreventsCompletion() async throws {
        let core = FakeTextTaggingCore(tasks: [makeTask("a")])
        let tagger = SuspendedSubjectTagger()
        let updates = AppliedBatchRecorder()
        let coordinator = TextTaggingCoordinator(tagger: tagger)
        let reconciliation = Task {
            try await coordinator.reconcile(core: core) { count in
                await updates.record(count)
            }
        }

        await tagger.waitUntilInferenceStarts()
        reconciliation.cancel()
        await tagger.finishInference()

        do {
            try await reconciliation.value
            XCTFail("Cancellation should stop the pass before it writes tags")
        } catch is CancellationError {
            // The coordinator checks cancellation again after inference returns.
        }

        let attempts = await core.completionAttempts()
        let published = await updates.counts()
        XCTAssertTrue(attempts.isEmpty)
        XCTAssertTrue(published.isEmpty)
    }

    private func makeTask(_ readingID: String) -> TextTaggingWorkItem {
        TextTaggingWorkItem(
            readingID: readingID,
            title: "Reading \(readingID)",
            text: "Content for \(readingID)",
            sourceFingerprint: "fingerprint-\(readingID)",
            analyzerVersion: "tagger-v1"
        )
    }
}

private enum FakeTextTaggingFailure: Error {
    case failed
}

private actor AppliedBatchRecorder {
    private var values: [Int] = []

    func record(_ value: Int) { values.append(value) }
    func counts() -> [Int] { values }
}

private actor FakeTextTaggingCore: TextTaggingCore {
    struct PendingCall: Equatable, Sendable {
        let analyzerVersion: String
        let limit: UInt32
        let afterReadingID: String?
    }

    struct Completion: Equatable, Sendable {
        let task: TextTaggingWorkItem
        let tags: [String]
    }

    private let tasks: [TextTaggingWorkItem]
    private let pageSize: Int
    private let staleReadingIDs: Set<String>
    private var pending: [PendingCall] = []
    private var attempted: [Completion] = []
    private var accepted: [Completion] = []

    init(
        tasks: [TextTaggingWorkItem],
        pageSize: Int = 8,
        staleReadingIDs: Set<String> = []
    ) {
        self.tasks = tasks.sorted { $0.readingID < $1.readingID }
        self.pageSize = pageSize
        self.staleReadingIDs = staleReadingIDs
    }

    func pendingTextTagging(
        analyzerVersion: String, limit: UInt32, afterReadingID: String?
    ) async throws -> TextTaggingBatch {
        pending.append(.init(
            analyzerVersion: analyzerVersion, limit: limit, afterReadingID: afterReadingID
        ))
        let remaining = tasks.filter { task in
            guard let afterReadingID else { return true }
            return task.readingID > afterReadingID
        }
        let page = Array(remaining.prefix(min(pageSize, Int(limit))))
        let nextReadingID = remaining.count > page.count ? page.last?.readingID : nil
        return TextTaggingBatch(tasks: page, nextReadingID: nextReadingID)
    }

    func completeTextTagging(task: TextTaggingWorkItem, tags: [String]) async throws -> Bool {
        let completion = Completion(task: task, tags: tags)
        attempted.append(completion)
        guard !staleReadingIDs.contains(task.readingID) else { return false }
        accepted.append(completion)
        return true
    }

    func pendingCalls() -> [PendingCall] { pending }
    func completionAttempts() -> [Completion] { attempted }
    func acceptedCompletions() -> [Completion] { accepted }
}

private actor FakeSubjectTagger: SubjectTagInferring {
    enum Outcome: Sendable {
        case labels([String])
        case failure
    }

    nonisolated let analyzerVersion = "tagger-v1"
    nonisolated let availability: SubjectTaggingAvailability
    private let outcomes: [String: Outcome]
    private var seenInputs: [String] = []

    init(
        availability: SubjectTaggingAvailability = .available,
        outcomes: [String: Outcome] = [:]
    ) {
        self.availability = availability
        self.outcomes = outcomes
    }

    func inferSubjectLabels(from text: String) async throws -> [String] {
        seenInputs.append(text)
        switch outcomes[text] ?? .labels([]) {
        case let .labels(labels):
            return labels
        case .failure:
            throw FakeTextTaggingFailure.failed
        }
    }

    func inputs() -> [String] { seenInputs }
}

private actor SuspendedSubjectTagger: SubjectTagInferring {
    nonisolated let analyzerVersion = "tagger-v1"
    nonisolated let availability: SubjectTaggingAvailability = .available
    private var inferenceContinuation: CheckedContinuation<Void, Never>?
    private var startWaiters: [CheckedContinuation<Void, Never>] = []

    func inferSubjectLabels(from _: String) async throws -> [String] {
        await withCheckedContinuation { continuation in
            inferenceContinuation = continuation
            let waiters = startWaiters
            startWaiters.removeAll()
            waiters.forEach { $0.resume() }
        }
        return ["architecture"]
    }

    func waitUntilInferenceStarts() async {
        guard inferenceContinuation == nil else { return }
        await withCheckedContinuation { continuation in
            startWaiters.append(continuation)
        }
    }

    func finishInference() {
        let continuation = inferenceContinuation
        inferenceContinuation = nil
        continuation?.resume()
    }
}

private final class MutableReadinessTagger: SubjectTagInferring {
    private struct State: Sendable {
        var availability: SubjectTaggingAvailability
    }

    let analyzerVersion = "tagger-v1"
    private let state: Mutex<State>

    init(availability: SubjectTaggingAvailability) {
        state = Mutex(State(availability: availability))
    }

    var availability: SubjectTaggingAvailability {
        state.withLock { $0.availability }
    }

    func setAvailability(_ availability: SubjectTaggingAvailability) {
        state.withLock { $0.availability = availability }
    }

    func inferSubjectLabels(from _: String) async throws -> [String] {
        ["architecture"]
    }
}

private actor RetryDelayRecorder {
    private var recorded: [Duration] = []

    func record(_ duration: Duration) {
        recorded.append(duration)
    }

    func delays() -> [Duration] { recorded }
}

private actor SuspendedRetryClock {
    private var sleepContinuation: CheckedContinuation<Void, Never>?
    private var startWaiters: [CheckedContinuation<Void, Never>] = []

    func sleep(for _: Duration) async {
        await withCheckedContinuation { continuation in
            sleepContinuation = continuation
            let waiters = startWaiters
            startWaiters.removeAll()
            waiters.forEach { $0.resume() }
        }
    }

    func waitUntilSleeping() async {
        guard sleepContinuation == nil else { return }
        await withCheckedContinuation { continuation in
            startWaiters.append(continuation)
        }
    }

    func finishSleep() {
        let continuation = sleepContinuation
        sleepContinuation = nil
        continuation?.resume()
    }
}
