// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

// ── Derived visual search ────────────────────────────────────────────────────
// Background visual analysis, automatic tags and Core Spotlight donation.
// Rust revalidates every result before writing tags to the local reading file.

extension AppState {
    func cancelVisualSearchReconciliation() -> Task<Void, Never>? {
        visualSearchRerunPending = false
        visualSearchTask?.cancel()
        return visualSearchTask
    }

    func scheduleVisualSearchReconciliation() {
        visualSearchRerunPending = true
        guard visualSearchTask == nil,
              let core,
              let visualSearchCoordinator,
              let coreID = activeCoreID else { return }
        let session = librarySessionGeneration

        visualSearchTask = Task(priority: .utility) { [weak self] in
            await self?.runVisualSearchReconciliations(
                core: core,
                coordinator: visualSearchCoordinator,
                coreID: coreID,
                session: session
            )
        }
    }

    private func runVisualSearchReconciliations(
        core: any CoreBridging,
        coordinator: VisualSearchCoordinator,
        coreID: ObjectIdentifier,
        session: UInt64
    ) async {
        repeat {
            guard isCurrentVisualSearchSession(coreID: coreID, session: session) else { break }
            visualSearchRerunPending = false
            do {
                async let textTagging: Void = reconcileTextTags(core: core, coreID: coreID, session: session)
                let result = try await coordinator.reconcile(core: core)
                guard !Task.isCancelled else { break }
                guard isCurrentVisualSearchSession(coreID: coreID, session: session) else { break }
                await visualSearchDidFinish(result)
                try await textTagging
            } catch is CancellationError {
                // Replacing a library deliberately supersedes this work.
                break
            } catch {
                // Visual indexing is an optional, rebuildable enhancement. A
                // corrupt image or unavailable system index must never make the
                // local library unusable or raise a blocking app alert.
            }
        } while visualSearchRerunPending && !Task.isCancelled

        visualSearchTask = nil
        if visualSearchRerunPending {
            scheduleVisualSearchReconciliation()
        }
    }

    private func isCurrentVisualSearchSession(
        coreID: ObjectIdentifier,
        session: UInt64
    ) -> Bool {
        !Task.isCancelled
            && session == librarySessionGeneration
            && coreID == activeCoreID
    }

    private func visualSearchDidFinish(_ result: VisualSearchReconciliation) async {
        if result.changedCardPresentation {
            visualAnalysisGeneration &+= 1
        }
        guard result.shouldReloadReadings(
            hasActiveSearch: hasSearchDependingOnVisualAnalysis
        ) else { return }
        async let filters: Void = loadFilters()
        let loadResult = await loadReadings(resetSelectionIfMissing: false)
        await filters
        guard loadResult == .published else { return }
        await visualSearchCoordinator?.acknowledgeAnalysisPresentation(
            result.analysisPublicationToken
        )
    }

    private func textTagsDidFinish(coreID: ObjectIdentifier, session: UInt64) async {
        guard isCurrentVisualSearchSession(coreID: coreID, session: session) else { return }
        await refresh()
    }

    private func reconcileTextTags(core: any CoreBridging, coreID: ObjectIdentifier, session: UInt64) async throws {
        guard let textTaggingCoordinator else { return }
        try await textTaggingCoordinator.reconcile(core: core) { [weak self] _ in
            await self?.textTagsDidFinish(coreID: coreID, session: session)
        }
    }

    private var hasSearchDependingOnVisualAnalysis: Bool {
        let search = activeSearchInput
        return search.text != nil || search.hasVisualTerms
    }
}
