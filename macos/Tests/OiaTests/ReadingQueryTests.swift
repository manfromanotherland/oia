// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class ReadingQueryTests: XCTestCase {
    private enum SearchFailure: Error { case unavailable }

    @MainActor
    private final class QueryGate {
        private var isOpen = false
        private var waiters: [CheckedContinuation<Void, Never>] = []

        func wait() async {
            guard !isOpen else { return }
            await withCheckedContinuation { waiters.append($0) }
        }

        func open() {
            isOpen = true
            let pending = waiters
            waiters.removeAll()
            for waiter in pending { waiter.resume() }
        }
    }

    @MainActor
    func testScopePublishesThePreviewWhileTheCompleteQueryIsPending() async throws {
        let fullStarted = QueryGate()
        let releaseFull = QueryGate()
        var limits: [UInt32] = []
        var publications: [[String]] = []
        var completions: [Bool] = []
        var returned = false
        let task = Task { @MainActor in
            let completed = try await ReadingSnapshotDelivery.loadScopeSnapshot(
                previewLimit: 2,
                fetch: { _, limit in
                    limits.append(limit)
                    if limit == 2 { return ["first", "second"] }
                    fullStarted.open()
                    await releaseFull.wait()
                    return ["first", "second", "third"]
                },
                semanticCandidates: nil,
                isCurrent: { true },
                publish: { rows, _, complete in
                    publications.append(rows)
                    completions.append(complete)
                }
            )
            returned = true
            return completed
        }

        await fullStarted.wait()
        XCTAssertEqual(publications, [["first", "second"]])
        XCTAssertEqual(limits, [2, UInt32.max])
        XCTAssertEqual(completions, [false])
        XCTAssertFalse(returned, "A visible preview does not complete the scope snapshot")

        releaseFull.open()
        let completed = try await task.value
        XCTAssertTrue(completed)
        XCTAssertEqual(publications, [["first", "second"], ["first", "second", "third"]])
        XCTAssertEqual(completions, [false, true])
    }

    @MainActor
    func testScopeWithFewerThanThePreviewLimitQueriesOnlyOnce() async throws {
        var limits: [UInt32] = []
        var publications: [[String]] = []
        var completions: [Bool] = []
        let completed = try await ReadingSnapshotDelivery.loadScopeSnapshot(
            fetch: { _, limit in limits.append(limit); return ["only row"] },
            semanticCandidates: nil,
            isCurrent: { true },
            publish: { rows, _, complete in
                publications.append(rows)
                completions.append(complete)
            }
        )

        XCTAssertTrue(completed)
        XCTAssertEqual(limits, [96])
        XCTAssertEqual(publications, [["only row"]])
        XCTAssertEqual(completions, [true])
    }

    @MainActor
    func testSupersededScopePreviewCannotPublishOrStartTheCompleteQuery() async throws {
        var current = true
        var limits: [UInt32] = []
        var publications: [[String]] = []
        let completed = try await ReadingSnapshotDelivery.loadScopeSnapshot(
            previewLimit: 1,
            fetch: { _, limit in
                limits.append(limit)
                current = false
                return ["stale preview"]
            },
            semanticCandidates: nil,
            isCurrent: { current },
            publish: { rows, _, _ in publications.append(rows) }
        )

        XCTAssertFalse(completed)
        XCTAssertEqual(limits, [1])
        XCTAssertTrue(publications.isEmpty)
    }

    @MainActor
    func testScopeSupersededAfterPublishingThePreviewDoesNotStartTheCompleteQuery() async throws {
        var current = true
        var limits: [UInt32] = []
        var publications: [[String]] = []
        let completed = try await ReadingSnapshotDelivery.loadScopeSnapshot(
            previewLimit: 1,
            fetch: { _, limit in limits.append(limit); return ["preview"] },
            semanticCandidates: nil,
            isCurrent: { current },
            publish: { rows, _, _ in
                publications.append(rows)
                current = false
            }
        )

        XCTAssertFalse(completed)
        XCTAssertEqual(limits, [1])
        XCTAssertEqual(publications, [["preview"]])
    }

    @MainActor
    func testSupersededCompleteScopeQueryCannotReplaceTheNewerSnapshot() async throws {
        let fullStarted = QueryGate()
        let releaseFull = QueryGate()
        var current = true
        var publications: [[String]] = []
        let task = Task { @MainActor in
            try await ReadingSnapshotDelivery.loadScopeSnapshot(
                previewLimit: 1,
                fetch: { _, limit in
                    if limit == 1 { return ["old preview"] }
                    fullStarted.open()
                    await releaseFull.wait()
                    return ["old preview", "old full result"]
                },
                semanticCandidates: nil,
                isCurrent: { current },
                publish: { rows, _, _ in publications.append(rows) }
            )
        }

        await fullStarted.wait()
        XCTAssertEqual(publications, [["old preview"]])
        current = false
        publications.append(["newer snapshot"])
        releaseFull.open()

        let completed = try await task.value
        XCTAssertFalse(completed)
        XCTAssertEqual(publications, [["old preview"], ["newer snapshot"]])
    }

    @MainActor
    func testCompleteScopeQueryFailureThrowsAfterPublishingThePreview() async throws {
        var limits: [UInt32] = []
        var publications: [[String]] = []
        var completions: [Bool] = []
        do {
            _ = try await ReadingSnapshotDelivery.loadScopeSnapshot(
                previewLimit: 1,
                fetch: { _, limit in
                    limits.append(limit)
                    if limit == 1 { return ["preview"] }
                    throw SearchFailure.unavailable
                },
                semanticCandidates: nil,
                isCurrent: { true },
                publish: { rows, _, complete in
                    publications.append(rows)
                    completions.append(complete)
                }
            )
            XCTFail("A failed complete query must not report a complete scope snapshot")
        } catch SearchFailure.unavailable {
            XCTAssertEqual(limits, [1, UInt32.max])
            XCTAssertEqual(publications, [["preview"]])
            XCTAssertEqual(completions, [false])
        }
    }

    @MainActor
    func testScopeResolvesSemanticCandidatesOnceBeforeBothQueries() async throws {
        let expected = ReadingSnapshotDelivery.Candidates(text: ["text match"], visual: ["visual match"])
        var semanticCalls = 0
        var candidatesUsed: [ReadingSnapshotDelivery.Candidates] = []
        var semanticFlags: [Bool] = []
        let completed = try await ReadingSnapshotDelivery.loadScopeSnapshot(
            previewLimit: 1,
            fetch: { candidates, limit in
                XCTAssertEqual(semanticCalls, 1)
                candidatesUsed.append(candidates)
                return limit == 1 ? ["preview"] : ["preview", "complete"]
            },
            semanticCandidates: {
                XCTAssertTrue(candidatesUsed.isEmpty)
                semanticCalls += 1
                return expected
            },
            isCurrent: { true },
            publish: { _, semantic, _ in semanticFlags.append(semantic) }
        )

        XCTAssertTrue(completed)
        XCTAssertEqual(semanticCalls, 1)
        XCTAssertEqual(candidatesUsed, [expected, expected])
        XCTAssertEqual(semanticFlags, [true, true])
    }

    @MainActor
    func testScopeSemanticFailureFallsBackToLocalCandidates() async throws {
        var candidatesUsed: [ReadingSnapshotDelivery.Candidates] = []
        var semanticFlags: [Bool] = []
        let completed = try await ReadingSnapshotDelivery.loadScopeSnapshot(
            fetch: { candidates, _ in candidatesUsed.append(candidates); return ["local"] },
            semanticCandidates: { throw SearchFailure.unavailable },
            isCurrent: { true },
            publish: { _, semantic, _ in semanticFlags.append(semantic) }
        )

        XCTAssertTrue(completed)
        XCTAssertEqual(candidatesUsed, [.empty])
        XCTAssertEqual(semanticFlags, [false])
    }

    @MainActor
    func testScopeSemanticCancellationDoesNotFetchOrPublish() async throws {
        var queryCount = 0
        var publications: [[String]] = []
        let completed = try await ReadingSnapshotDelivery.loadScopeSnapshot(
            fetch: { _, _ in queryCount += 1; return ["stale"] },
            semanticCandidates: { throw CancellationError() },
            isCurrent: { true },
            publish: { rows, _, _ in publications.append(rows) }
        )

        XCTAssertFalse(completed)
        XCTAssertEqual(queryCount, 0)
        XCTAssertTrue(publications.isEmpty)
    }

    @MainActor
    func testSupersededScopeSemanticResultDoesNotFetchOrPublish() async throws {
        var current = true
        var queryCount = 0
        var publications: [[String]] = []
        let completed = try await ReadingSnapshotDelivery.loadScopeSnapshot(
            fetch: { _, _ in queryCount += 1; return ["stale"] },
            semanticCandidates: {
                current = false
                return .init(text: ["stale semantic"])
            },
            isCurrent: { current },
            publish: { rows, _, _ in publications.append(rows) }
        )

        XCTAssertFalse(completed)
        XCTAssertEqual(queryCount, 0)
        XCTAssertTrue(publications.isEmpty)
    }

    @MainActor
    func testAlreadyCancelledScopeDoesNotStartQueries() async throws {
        var queryCount = 0
        var semanticCalls = 0
        var publications: [[String]] = []
        let task = Task { @MainActor in
            try await ReadingSnapshotDelivery.loadScopeSnapshot(
                fetch: { _, _ in queryCount += 1; return ["cancelled"] },
                semanticCandidates: { semanticCalls += 1; return .empty },
                isCurrent: { true },
                publish: { rows, _, _ in publications.append(rows) }
            )
        }
        task.cancel()

        let completed = try await task.value
        XCTAssertFalse(completed)
        XCTAssertEqual(queryCount, 0)
        XCTAssertEqual(semanticCalls, 0)
        XCTAssertTrue(publications.isEmpty)
    }

    @MainActor
    func testCancelledScopePreviewCannotPublishOrStartTheCompleteQuery() async throws {
        let previewStarted = QueryGate()
        let releasePreview = QueryGate()
        var limits: [UInt32] = []
        var publications: [[String]] = []
        let task = Task { @MainActor in
            try await ReadingSnapshotDelivery.loadScopeSnapshot(
                previewLimit: 1,
                fetch: { _, limit in
                    limits.append(limit)
                    previewStarted.open()
                    await releasePreview.wait()
                    return ["cancelled preview"]
                },
                semanticCandidates: nil,
                isCurrent: { true },
                publish: { rows, _, _ in publications.append(rows) }
            )
        }

        await previewStarted.wait()
        task.cancel()
        releasePreview.open()

        let completed = try await task.value
        XCTAssertFalse(completed)
        XCTAssertEqual(limits, [1])
        XCTAssertTrue(publications.isEmpty)
    }

    @MainActor
    func testCancelledCompleteScopeQueryCannotPublishItsResult() async throws {
        let fullStarted = QueryGate()
        let releaseFull = QueryGate()
        var publications: [[String]] = []
        let task = Task { @MainActor in
            try await ReadingSnapshotDelivery.loadScopeSnapshot(
                previewLimit: 1,
                fetch: { _, limit in
                    if limit == 1 { return ["preview"] }
                    fullStarted.open()
                    await releaseFull.wait()
                    return ["preview", "cancelled full result"]
                },
                semanticCandidates: nil,
                isCurrent: { true },
                publish: { rows, _, _ in publications.append(rows) }
            )
        }

        await fullStarted.wait()
        task.cancel()
        releaseFull.open()

        let completed = try await task.value
        XCTAssertFalse(completed)
        XCTAssertEqual(publications, [["preview"]])
    }

    @MainActor
    func testRefreshKeepsSemanticMembershipUntilTheCombinedResultIsReady() async throws {
        var queries: [[String]] = []
        var publications: [[String]] = []
        let completed = try await ReadingSnapshotDelivery.load(
            textFirst: false,
            fetch: { candidates in
                queries.append(candidates.text)
                return ["text"] + candidates.text
            },
            semanticCandidates: {
                XCTAssertTrue(publications.isEmpty)
                return .init(text: ["semantic"])
            },
            isCurrent: { true },
            publish: { rows, _ in publications.append(rows) }
        )
        XCTAssertTrue(completed)
        XCTAssertEqual(queries, [["semantic"]])
        XCTAssertEqual(publications, [["text", "semantic"]])
    }

    @MainActor
    func testSemanticFailureKeepsPublishedTextResults() async throws {
        var publications: [[String]] = []
        var queryCount = 0
        let completed = try await ReadingSnapshotDelivery.load(
            fetch: { _ in queryCount += 1; return ["text"] },
            semanticCandidates: { throw SearchFailure.unavailable },
            isCurrent: { true },
            publish: { rows, _ in publications.append(rows) }
        )
        XCTAssertTrue(completed)
        XCTAssertEqual(queryCount, 1)
        XCTAssertEqual(publications, [["text"]])
    }

    @MainActor
    func testEmptySemanticCandidatesDoNotRepeatTheCoreQuery() async throws {
        var queryCount = 0
        let completed = try await ReadingSnapshotDelivery.load(
            fetch: { _ in queryCount += 1; return ["text"] },
            semanticCandidates: { .empty },
            isCurrent: { true },
            publish: { _, _ in }
        )
        XCTAssertTrue(completed)
        XCTAssertEqual(queryCount, 1)
    }

    @MainActor
    func testSupersededSemanticResultCannotReplaceTheNewerQuery() async throws {
        var current = true
        var publications: [[String]] = []
        var queryCount = 0
        let completed = try await ReadingSnapshotDelivery.load(
            fetch: { _ in queryCount += 1; return ["text"] },
            semanticCandidates: {
                current = false
                return .init(text: ["stale semantic"])
            },
            isCurrent: { current },
            publish: { rows, _ in publications.append(rows) }
        )
        XCTAssertFalse(completed)
        XCTAssertEqual(queryCount, 1)
        XCTAssertEqual(publications, [["text"]])
    }

    @MainActor
    func testSupersededTextQueryCannotPublishOrStartSemanticWork() async throws {
        var current = true
        var publications: [[String]] = []
        var semanticStarted = false
        let completed = try await ReadingSnapshotDelivery.load(
            fetch: { _ in current = false; return ["stale text"] },
            semanticCandidates: { semanticStarted = true; return .empty },
            isCurrent: { current },
            publish: { rows, _ in publications.append(rows) }
        )
        XCTAssertFalse(completed)
        XCTAssertFalse(semanticStarted)
        XCTAssertTrue(publications.isEmpty)
    }

    @MainActor
    func testSemanticCancellationPreservesTextButDoesNotCompleteTheGeneration() async throws {
        var publications: [[String]] = []
        let completed = try await ReadingSnapshotDelivery.load(
            fetch: { _ in ["text"] },
            semanticCandidates: { throw CancellationError() },
            isCurrent: { true },
            publish: { rows, _ in publications.append(rows) }
        )
        XCTAssertFalse(completed)
        XCTAssertEqual(publications, [["text"]])
    }

    @MainActor
    func testTextResultsPublishBeforeSemanticSearchStarts() async throws {
        var publications: [[String]] = []
        var queries: [[String]] = []
        let completed = try await ReadingSnapshotDelivery.load(
            fetch: { candidates in
                queries.append(candidates.text)
                return candidates.isEmpty ? ["text"] : ["text", "semantic"]
            },
            semanticCandidates: {
                XCTAssertEqual(publications, [["text"]])
                return .init(text: ["semantic"])
            },
            isCurrent: { true },
            publish: { rows, _ in publications.append(rows) }
        )

        XCTAssertTrue(completed)
        XCTAssertEqual(queries, [[], ["semantic"]])
        XCTAssertEqual(publications, [["text"], ["text", "semantic"]])
    }

    func testSearchKeepsEverySelectedBoardScope() {
        for scope in LibraryScope.allCases {
            let query = ReadingQuery.boardSnapshot(
                scope: scope,
                search: "texture",
                tagTerms: ["interiors"],
                visualTerms: ["blue", "furniture"],
                semanticCandidateIDs: ["first", "second"],
                visualSemanticCandidateIDs: ["visual"]
            )

            XCTAssertEqual(query.scope, scope)
            XCTAssertEqual(query.search, "texture")
            XCTAssertEqual(query.tagTerms, ["interiors"])
            XCTAssertEqual(query.visualTerms, ["blue", "furniture"])
            XCTAssertEqual(query.semanticCandidateIDs, ["first", "second"])
            XCTAssertEqual(query.visualSemanticCandidateIDs, ["visual"])
            XCTAssertEqual(query.limit, .max)
            XCTAssertEqual(query.offset, 0)
            XCTAssertFalse(query.ascending)
            XCTAssertEqual(query.kind, scope.readingKindFilter)
            XCTAssertNil(query.tag)
            guard case .relevance = query.sort else {
                return XCTFail("search should use relevance ordering")
            }
        }
    }

    func testBrowsingUsesNewestFirstOrdering() {
        let query = ReadingQuery.boardSnapshot(
            scope: .images,
            search: nil,
            tagTerms: [],
            visualTerms: [],
            semanticCandidateIDs: [],
            visualSemanticCandidateIDs: []
        )

        XCTAssertEqual(query.scope, .images)
        XCTAssertEqual(query.kind, .image)
        XCTAssertNil(query.search)
        XCTAssertFalse(query.ascending)
        guard case .savedAt = query.sort else {
            return XCTFail("browsing should use saved-date ordering")
        }
    }

    func testMediaScopesUseCoreAllViewAndDistinctKindFilters() {
        let scopes: [(LibraryScope, FfiReadingKind)] = [
            (.images, .image),
            (.videos, .video)
        ]

        for (scope, kind) in scopes {
            let query = ReadingQuery.boardSnapshot(
                scope: scope,
                search: "texture",
                tagTerms: ["interiors"],
                visualTerms: ["blue"],
                semanticCandidateIDs: ["match"],
                visualSemanticCandidateIDs: ["visual"]
            )

            XCTAssertEqual(scope.ffiView, .all)
            XCTAssertEqual(query.kind?.ffiKind, kind)
            XCTAssertEqual(query.tagTerms, ["interiors"])
            XCTAssertEqual(query.visualTerms, ["blue"])
            XCTAssertEqual(query.semanticCandidateIDs, ["match"])
            XCTAssertEqual(query.visualSemanticCandidateIDs, ["visual"])
        }
    }

    func testStructuredTermsUseSearchOrderingWithoutFreeText() {
        let query = ReadingQuery.boardSnapshot(
            scope: .all,
            search: nil,
            tagTerms: ["chairs"],
            visualTerms: ["blue", "furniture"],
            semanticCandidateIDs: [],
            visualSemanticCandidateIDs: []
        )

        XCTAssertEqual(query.tagTerms, ["chairs"])
        XCTAssertEqual(query.visualTerms, ["blue", "furniture"])
        guard case .relevance = query.sort else {
            return XCTFail("structured search should use relevance ordering")
        }
    }

    func testColorTokenUsesSearchOrderingWithoutVisibleQueryText() {
        let query = ReadingQuery.boardSnapshot(
            scope: .all,
            search: nil,
            tagTerms: [],
            visualTerms: [],
            colorTerms: ["#42C878"],
            semanticCandidateIDs: [],
            visualSemanticCandidateIDs: []
        )

        XCTAssertNil(query.search)
        XCTAssertEqual(query.colorTerms, ["#42C878"])
        guard case .relevance = query.sort else {
            return XCTFail("color tokens should use search ordering")
        }
    }

    func testVisualSemanticCandidatesRequireEveryTermOnTheSameImage() {
        let candidates = VisualSemanticCandidateIntersection.ranked([
            ["blue-only", "same-image", "same-image"],
            ["furniture-only", "same-image"]
        ])

        XCTAssertEqual(candidates, ["same-image"])
    }
}
