// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Foundation

private struct LibraryBootRecovery {
    var needsRetry = false
    var retryImmediately = false
}
// ── Library lifecycle ────────────────────────────────────────────────────────
// Choosing/restoring the library folder, booting the core against it, and
// keeping the index in sync with on-disk changes (FSEvents).

extension AppState {
    // ── Onboarding ────────────────────────────────────────────────────────

    func chooseLibrary() {
        guard canChangeLibrary else { return }
        // UI-testing: pick the scripted folder directly — NSOpenPanel is a
        // system dialog XCUITest can't reliably drive.
        if let path = TestHooks.onboardingPickPath {
            Task { await pickLibrary(url: URL(fileURLWithPath: path)) }
            return
        }
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.prompt = "Choose Library Folder"
        panel.message = "Choose a folder for your library."
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await pickLibrary(url: url) }
    }

    private func pickLibrary(url: URL) async {
        guard canChangeLibrary else { return }
        do {
            try LibrarySetup.scaffold(at: url)
            WelcomeArticle.seedIfEmpty(in: url)
            // Keep scaffolding + welcome seed so onboarding stays testable, but
            // in UI-testing mode skip the bookmark write so
            // the dev's persisted library bookmark is never overwritten.
            if !TestHooks.isUITesting {
                try LibraryBookmark.save(url: url)
            }
            await boot(url: url)
        } catch {
            self.error = error.localizedDescription
        }
    }

    // ── Boot ──────────────────────────────────────────────────────────────

    func boot(url: URL, allowsImmediateRecoveryRetry: Bool = true) async {
        guard canChangeLibrary else { return }

        let previousVisualSearchTask = cancelVisualSearchReconciliation()
        hasUsableCachedLibrary = false
        isReconcilingLibrary = true
        isLoading = true
        error = nil
        let previousSyncTask = beginLibrarySession(at: url)
        let session = librarySessionGeneration
        var recovery = LibraryBootRecovery()
        defer {
            finishLibraryBoot(libraryURL: url, session: session, recovery: recovery)
        }

        // Give SwiftUI one main-actor turn to paint the restored window shell
        // before SQLite open/migration or any library work begins.
        await Task.yield()
        await previousSyncTask?.value
        await previousVisualSearchTask?.value

        do {
            try await openAndReconcileLibrary(at: url, session: session)
        } catch {
            // Retry once before relying on FSEvents, which may still be coalescing.
            recovery.retryImmediately = allowsImmediateRecoveryRetry
            recovery.needsRetry = handleLibraryBootFailure(error)
        }
    }
    private func finishLibraryBoot(
        libraryURL: URL,
        session: UInt64,
        recovery: LibraryBootRecovery
    ) {
        isLoading = false
        isRestoringLibrary = false
        isReconcilingLibrary = false
        hasUsableCachedLibrary = false
        if recovery.needsRetry {
            startRebuildRetryWatcher(
                libraryURL: libraryURL,
                session: session,
                retryImmediately: recovery.retryImmediately
            )
        } else {
            // Publish the existing board before starting any Inbox work. This
            // also picks up captures that arrived while Óia was closed.
            watcherSyncPending = true
            watcherChanges.merge(.full)
            scheduleWatcherSyncIfNeeded(session: session)
        }
    }

    private func beginLibrarySession(at url: URL) -> Task<Void, Never>? {
        librarySessionGeneration &+= 1
        watcher?.invalidate()
        watcher = nil
        watcherSyncPending = false
        watcherChanges = FolderWatcher.Change()
        inboxRetryTask?.cancel()
        inboxRetryTask = nil
        inboxRetryAttempt = 0
        isProcessingInbox = false
        inboxPendingCount = 0
        inboxIssues = []
        inboxError = nil
        searchTask?.cancel()
        searchTask = nil
        readingLoadGeneration &+= 1
        libraryContentRefreshPending = false
        let previousSyncTask = watcherSyncTask
        previousSyncTask?.cancel()
        watcherSyncTask = nil
        activeCoreID = nil

        if libraryURL?.standardizedFileURL.path != url.standardizedFileURL.path {
            core = nil
            readings = []
            boardSelection.clear()
            filters = LibraryFilters()
        }
        libraryURL = url
        return previousSyncTask
    }

    private func openAndReconcileLibrary(at url: URL, session: UInt64) async throws {
        try await waitForTestLibraryHydration()

        let databasePath = Self.dbPath()
        let cacheStatus = IndexCacheTrust.status(
            libraryURL: url,
            databasePath: databasePath
        )
        let openedBridge = try await CoreBridge.open(
            libraryPath: url.path,
            dbPath: databasePath
        )
        let useCachedIndex = await canPresentCachedIndex(
            status: cacheStatus,
            libraryURL: url,
            bridge: openedBridge
        )

        if useCachedIndex {
            await presentCachedLibrary(at: url, using: openedBridge)
        }

        // Keep cached reads and interactions on their own actor while the full
        // file scan occupies a second connection. WAL lets the cached board stay
        // responsive until the rebuilt snapshot is ready to publish.
        let reconciliationBridge = try await makeReconciliationBridge(
            openedBridge: openedBridge,
            useCachedIndex: useCachedIndex,
            libraryURL: url,
            databasePath: databasePath
        )
        try await reconcileLibrary(
            at: url,
            databasePath: databasePath,
            using: reconciliationBridge,
            session: session
        )
    }

    private func waitForTestLibraryHydration() async throws {
        if let delay = TestHooks.libraryHydrationDelayNanoseconds {
            try await Task<Never, Never>.sleep(nanoseconds: delay)
        }
    }

    private func canPresentCachedIndex(
        status: IndexCacheTrust.Status,
        libraryURL: URL,
        bridge: CoreBridge
    ) async -> Bool {
        switch status {
        case .trusted:
            return true
        case .legacy:
            return await IndexCacheTrust.legacyCacheMatches(
                libraryURL: libraryURL,
                bridge: bridge
            )
        case .unavailable:
            return false
        }
    }

    private func makeReconciliationBridge(
        openedBridge: CoreBridge,
        useCachedIndex: Bool,
        libraryURL: URL,
        databasePath: String
    ) async throws -> CoreBridge {
        guard useCachedIndex else { return openedBridge }
        return try await CoreBridge.open(
            libraryPath: libraryURL.path,
            dbPath: databasePath
        )
    }

    private func presentCachedLibrary(at url: URL, using bridge: CoreBridge) async {
        core = bridge
        activeCoreID = ObjectIdentifier(bridge)
        libraryURL = url
        // Text/label search is already in SQLite. Do not hold the cached first
        // frame on a fresh Core Spotlight query for a persisted search term.
        let loadResult = await loadReadings(includeSemanticSearch: false)
        hasUsableCachedLibrary = loadResult != .failed
        guard hasUsableCachedLibrary else { return }

        isLoading = false
        isRestoringLibrary = false
        TestHooks.recordStartupEvent("cached-readings")
        if loadResult == .published {
            TestHooks.recordStartupSnapshot("cached-snapshot", rows: readings)
        }
    }

    private func reconcileLibrary(
        at url: URL,
        databasePath: String,
        using bridge: CoreBridge,
        session: UInt64
    ) async throws {
        // Existing libraries predate Inbox. Scaffold off the main actor before
        // registering the watcher, after any trusted cache has been published.
        await Task.detached(priority: .utility) {
            // Inbox errors are reported independently by its first check. They
            // must not keep the existing library from opening.
            try? LibrarySetup.scaffold(at: url)
        }.value
        // Start FSEvents before the scan. Changes that land during rebuild are
        // coalesced and synced once the rebuilt snapshot becomes active.
        startWatcher(libraryPath: url.path, session: session)
        TestHooks.recordStartupEvent("reconcile-started")
        if let delay = TestHooks.libraryReconciliationDelayNanoseconds {
            try await Task<Never, Never>.sleep(nanoseconds: delay)
        }

        // Files remain authoritative. The cache only removes the startup wait.
        try await bridge.rebuild()
        core = bridge
        activeCoreID = ObjectIdentifier(bridge)
        libraryURL = url
        libraryContentRefreshPending = true

        let loadResult = await loadReadings()
        if loadResult == .failed {
            throw NSError(
                domain: "is.edmundo.cuttings.library",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: error ?? "Unable to load the rebuilt library."]
            )
        }
        isLoading = false
        isRestoringLibrary = false
        if loadResult == .published {
            TestHooks.recordStartupEvent("reconciled-readings")
            TestHooks.recordStartupSnapshot("reconciled-snapshot", rows: readings)
            await Task.yield()
        }

        performPostReconciliationWork(libraryURL: url, databasePath: databasePath)
        TestHooks.recordStartupEvent("reconcile-finished")
    }
    private func performPostReconciliationWork(libraryURL: URL, databasePath: String) {
        // Host-machine side effects are neutralized under UI testing so runs do
        // not touch the real machine's config or browser manifests.
        if !TestHooks.isIsolatedRun {
            writeLibraryPathConfig(libraryURL.path)
            NativeHostInstaller.install()
        }
        IndexCacheTrust.record(libraryURL: libraryURL, databasePath: databasePath)

        // Tags are not needed to draw the board. Load their large global
        // vocabulary only after the first cards are free to render.
        Task { [weak self] in await self?.loadFilters() }
        scheduleVisualSearchReconciliation()
    }

    private func handleLibraryBootFailure(_ bootError: any Error) -> Bool {
        error = bootError.localizedDescription
        TestHooks.recordStartupEvent("startup-error", details: bootError.localizedDescription)
        watcher?.invalidate()
        watcher = nil
        watcherSyncPending = false
        watcherChanges = FolderWatcher.Change()
        guard !hasUsableCachedLibrary else { return true }

        activeCoreID = nil
        core = nil
        readings = []
        boardSelection.clear()
        filters = LibraryFilters()
        libraryURL = nil
        return false
    }

    /// Write the library path to ~/.config/cuttings/library so the native
    /// messaging host can find it without needing a security-scoped bookmark.
    private func writeLibraryPathConfig(_ path: String) {
        let configURL = Self.libraryPathConfigURL
        let configDir = configURL.deletingLastPathComponent()
        try? FileManager.default.createDirectory(
            at: configDir, withIntermediateDirectories: true
        )
        try? (path + "\n").write(
            to: configURL,
            atomically: true, encoding: .utf8
        )
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private static var libraryPathConfigURL: URL {
        URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent(".config/cuttings/library")
    }

    static func dbPath() -> String {
        // UI-testing: use the pinned temp DB so the real per-device index is
        // never opened or mutated.
        if let path = TestHooks.dbPath {
            return path
        }
        let support = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Cuttings", isDirectory: true)
        try? FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        return support.appendingPathComponent("index.db").path
    }
}
