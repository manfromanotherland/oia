// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Foundation

// ── Incremental sync (FSEvents) ─────────────────────────────────────────────

extension AppState {
    private func requestWatcherSync(session: UInt64, change: FolderWatcher.Change = .full) {
        guard session == librarySessionGeneration else { return }
        watcherSyncPending = true
        watcherChanges.merge(change)
        scheduleWatcherSyncIfNeeded(session: session)
    }

    func scheduleWatcherSyncIfNeeded(session: UInt64) {
        guard session == librarySessionGeneration,
              !isReconcilingLibrary,
              watcherSyncPending,
              watcherSyncTask == nil,
              let bridge = core as? CoreBridge,
              activeCoreID == ObjectIdentifier(bridge) else { return }

        watcherSyncPending = false
        let changes = watcherChanges
        watcherChanges = FolderWatcher.Change()
        watcherSyncTask = Task(priority: .utility) { @MainActor [weak self] in
            guard let self else { return }
            await sync(using: bridge, session: session, changes: changes)
            finishWatcherSync(session: session)
        }
    }

    private func finishWatcherSync(session: UInt64) {
        guard session == librarySessionGeneration else { return }
        watcherSyncTask = nil
        scheduleWatcherSyncIfNeeded(session: session)
    }

    private func sync(using bridge: CoreBridge, session: UInt64, changes: FolderWatcher.Change) async {
        guard session == librarySessionGeneration,
              activeCoreID == ObjectIdentifier(bridge),
              !Task.isCancelled else { return }
        let bridgeID = ObjectIdentifier(bridge)
        let savedInboxItems = await processInbox(using: bridge, session: session)
        guard session == librarySessionGeneration,
              activeCoreID == bridgeID,
              !Task.isCancelled else { return }
        do {
            let changed: UInt32 = if changes.requiresFullScan || savedInboxItems {
                try await bridge.sync()
            } else {
                try await bridge.sync(paths: Array(changes.paths))
            }
            guard session == librarySessionGeneration,
                  activeCoreID == bridgeID,
                  !Task.isCancelled else { return }
            if changed > 0 || changes.requiresFullScan {
                await refresh()
                scheduleVisualSearchReconciliation()
            }
        } catch {
            if session == librarySessionGeneration,
               activeCoreID == bridgeID,
               !Task.isCancelled
            {
                self.error = error.localizedDescription
            }
        }
    }

    private func processInbox(using bridge: CoreBridge, session: UInt64) async -> Bool {
        isProcessingInbox = true
        beginLibraryWrite()
        defer {
            endLibraryWrite()
            if session == librarySessionGeneration {
                isProcessingInbox = false
            }
        }
        do {
            let report = try await bridge.processInbox()
            guard session == librarySessionGeneration,
                  activeCoreID == ObjectIdentifier(bridge),
                  !Task.isCancelled else { return false }
            inboxPendingCount = report.pending
            inboxIssues = report.issues
            inboxError = nil
            scheduleInboxRetry(session: session, pending: report.pending)
            if report.saved > 0 {
                let message = report.saved == 1
                    ? "Saved 1 item from Inbox"
                    : "Saved \(report.saved) items from Inbox"
                presentSaveNotice(message, systemImage: "checkmark.circle.fill")
            }
            return report.saved > 0
        } catch {
            guard session == librarySessionGeneration,
                  activeCoreID == ObjectIdentifier(bridge),
                  !Task.isCancelled else { return false }
            // A broken Inbox must not prevent ordinary library reconciliation.
            // Keep persistent feedback beside its controls instead of showing
            // the same modal error on every provider event.
            inboxError = error.localizedDescription
            scheduleInboxRetry(session: session, pending: 0)
            return false
        }
    }

    private func scheduleInboxRetry(session: UInt64, pending: UInt32) {
        guard pending > 0 else {
            inboxRetryTask?.cancel()
            inboxRetryTask = nil
            inboxRetryAttempt = 0
            return
        }
        guard inboxRetryTask == nil else { return }
        let delay = InboxRetrySchedule.delay(attempt: inboxRetryAttempt)
        inboxRetryAttempt = min(inboxRetryAttempt + 1, 3)
        inboxRetryTask = Task { @MainActor [weak self] in
            do { try await Task.sleep(for: delay) } catch { return }
            guard let self,
                  session == librarySessionGeneration,
                  !Task.isCancelled else { return }
            inboxRetryTask = nil
            requestWatcherSync(session: session)
        }
    }

    func checkInbox() {
        inboxRetryTask?.cancel()
        inboxRetryTask = nil
        inboxRetryAttempt = 0
        requestWatcherSync(session: librarySessionGeneration)
    }

    func openInbox() {
        guard let libraryURL else { return }
        NSWorkspace.shared.open(libraryURL.appendingPathComponent("inbox", isDirectory: true))
    }

    func startWatcher(libraryPath: String, session: UInt64) {
        guard session == librarySessionGeneration else { return }
        // Cancel previous monitoring before replacement. Session guards also
        // prevent late callbacks from reconciling a different library.
        watcher?.invalidate()
        watcher = FolderWatcher(libraryPath: libraryPath) { [weak self] change in
            Task { @MainActor [weak self] in
                self?.requestWatcherSync(session: session, change: change)
            }
        }
    }

    func startRebuildRetryWatcher(
        libraryURL: URL,
        session: UInt64,
        retryImmediately: Bool
    ) {
        guard session == librarySessionGeneration else { return }
        watcher?.invalidate()
        // Recovery waits for a real file event; startup reconciliation would
        // turn a persistent scan failure into an immediate retry loop.
        watcher = FolderWatcher(libraryPath: libraryURL.path, reconcileOnStart: false) { [weak self] _ in
            Task { @MainActor [weak self] in
                await self?.retryFullRebuildWhenReady(
                    libraryURL: libraryURL,
                    session: session,
                    allowsImmediateRecoveryRetry: true
                )
            }
        }
        guard retryImmediately else { return }
        Task { @MainActor [weak self] in
            await self?.retryFullRebuildWhenReady(
                libraryURL: libraryURL,
                session: session,
                allowsImmediateRecoveryRetry: false
            )
        }
    }

    private func retryFullRebuildWhenReady(
        libraryURL: URL,
        session: UInt64,
        allowsImmediateRecoveryRetry: Bool
    ) async {
        while session == librarySessionGeneration,
              activeLibraryWriteCount > 0 || isSaving
        {
            try? await Task.sleep(for: .milliseconds(100))
        }
        guard session == librarySessionGeneration, canChangeLibrary else { return }
        await boot(
            url: libraryURL,
            allowsImmediateRecoveryRetry: allowsImmediateRecoveryRetry
        )
    }
}
