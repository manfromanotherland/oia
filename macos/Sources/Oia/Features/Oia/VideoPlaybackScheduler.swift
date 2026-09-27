// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// One scheduler per board, with cancellable FIFO admission and spaced preparation.
/// A lease belongs to a retained player, so scrolling can pause without rebuilding it.
@MainActor
final class VideoPlaybackScheduler {
    private let limit: Int
    private let startSpacing: Duration
    private var active: Set<UUID> = []
    private var waiting: [UUID: CheckedContinuation<UUID?, Never>] = [:]
    private var order: [UUID] = []
    private var nextStart: ContinuousClock.Instant?

    init(limit: Int = .max, startSpacing: Duration = .milliseconds(90)) {
        self.limit = max(1, limit)
        self.startSpacing = startSpacing
    }

    func acquire() async -> UUID? {
        guard !Task.isCancelled else { return nil }
        let id = UUID()
        if active.count < limit {
            active.insert(id)
            return id
        }
        let admitted: UUID? = await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                guard !Task.isCancelled else {
                    continuation.resume(returning: nil)
                    return
                }
                waiting[id] = continuation
                order.append(id)
            }
        } onCancel: {
            Task { @MainActor in self.cancel(id) }
        }
        guard !Task.isCancelled else {
            release(id)
            return nil
        }
        return admitted
    }

    func release(_ id: UUID) {
        guard active.remove(id) != nil else { return }
        while !order.isEmpty {
            let next = order.removeFirst()
            guard let continuation = waiting.removeValue(forKey: next) else { continue }
            active.insert(next)
            continuation.resume(returning: next)
            return
        }
    }

    func waitForStartTurn() async -> Bool {
        let now = ContinuousClock.now
        let start = max(now, nextStart ?? now)
        nextStart = start.advanced(by: startSpacing)
        do {
            try await Task.sleep(until: start, clock: .continuous)
            return !Task.isCancelled
        } catch {
            return false
        }
    }

    var state: State {
        State(active: active.count, waiting: waiting.count)
    }

    struct State: Equatable {
        let active: Int
        let waiting: Int
    }

    private func cancel(_ id: UUID) {
        waiting.removeValue(forKey: id)?.resume(returning: nil)
        order.removeAll { $0 == id }
        // Cancellation can race with admission, before acquire returns its lease.
        release(id)
    }
}
