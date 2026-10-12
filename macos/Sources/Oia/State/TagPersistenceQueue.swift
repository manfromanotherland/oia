// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Orders tag persistence across editor lifetimes. Reservation is synchronous;
/// the submitted work keeps running after its initiating view is dismissed.
@MainActor
final class TagPersistenceQueue {
    private var tail: Task<Int, Never>?
    private var generation: UInt64 = 0

    var isBusy: Bool {
        tail != nil
    }

    @discardableResult
    func enqueue(_ operation: @escaping @MainActor () async -> Int) -> Task<Int, Never> {
        let previous = tail
        generation &+= 1
        let reservation = generation
        let task = Task { [self] in
            defer {
                if generation == reservation {
                    tail = nil
                }
            }
            _ = await previous?.value
            return await operation()
        }
        tail = task
        return task
    }
}
