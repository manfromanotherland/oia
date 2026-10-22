// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A lifecycle seam for testing slow native stream creation without blocking macOS.
protocol FolderWatcherStream: Sendable {
    func start() -> Bool
    func stop()
}

/// Watches `articles/` and `inbox/` inside the library root with FSEvents and invokes
/// `onChange` whenever files are created, modified, or deleted — including
/// files arriving via iCloud or other sync services.
///
/// The FSEvents latency (0.5 s) coalesces rapid bursts into a single callback,
/// so the caller doesn't need its own debounce.
final class FolderWatcher: @unchecked Sendable {
    struct Change: Equatable, Sendable {
        var paths: Set<String> = []
        var requiresFullScan = false

        static let full = Change(requiresFullScan: true)

        mutating func merge(_ other: Change) {
            paths.formUnion(other.paths)
            requiresFullScan = requiresFullScan || other.requiresFullScan
        }

        static func events(paths: [String], flags: [FSEventStreamEventFlags]) -> Change {
            let recoveryFlags = FSEventStreamEventFlags(
                kFSEventStreamEventFlagMustScanSubDirs | kFSEventStreamEventFlagUserDropped
                    | kFSEventStreamEventFlagKernelDropped | kFSEventStreamEventFlagEventIdsWrapped
                    | kFSEventStreamEventFlagRootChanged | kFSEventStreamEventFlagMount
                    | kFSEventStreamEventFlagUnmount
            )
            return Change(
                paths: Set(paths),
                requiresFullScan: paths.count != flags.count
                    || flags.contains { $0 & recoveryFlags != 0 }
            )
        }
    }

    typealias StreamFactory = @Sendable (
        [String], DispatchQueue, @escaping @Sendable (Change) -> Void
    ) -> (any FolderWatcherStream)?

    private let lifecycle: Lifecycle

    convenience init(libraryPath: String, reconcileOnStart: Bool = true,
                     onChange: @escaping @Sendable (Change) -> Void) {
        self.init(paths: Self.watchPaths(libraryPath: libraryPath), reconcileOnStart: reconcileOnStart,
                  onChange: onChange) {
            NativeFolderWatcherStream(paths: $0, queue: $1, onChange: $2)
        }
    }

    init(paths: [String], reconcileOnStart: Bool = true,
         onChange: @escaping @Sendable (Change) -> Void,
         streamFactory: @escaping StreamFactory) {
        lifecycle = Lifecycle(reconcileOnStart: reconcileOnStart, onChange: onChange,
                              streamFactory: streamFactory)
        lifecycle.start(paths: paths)
    }

    static func watchPaths(libraryPath: String) -> [String] {
        LibrarySetup.subdirectories.map { (libraryPath as NSString).appendingPathComponent($0) }
    }

    deinit { lifecycle.invalidate() }

    /// Stop monitoring this library.
    func invalidate() {
        lifecycle.invalidate()
    }

    /// Stream ownership never crosses this serial queue. The cancellation gate
    /// stays independent, so invalidation cannot wait for a blocked native open.
    private final class Lifecycle: @unchecked Sendable {
        private let queue = DispatchQueue(label: "is.edmundo.cuttings.fsevents", qos: .utility)
        private let cancellationLock = NSLock()
        private var invalidated = false
        private var stream: (any FolderWatcherStream)?
        private let onChange: @Sendable (Change) -> Void
        private let streamFactory: StreamFactory
        private let reconcileOnStart: Bool

        init(reconcileOnStart: Bool, onChange: @escaping @Sendable (Change) -> Void,
             streamFactory: @escaping StreamFactory) {
            self.onChange = onChange
            self.streamFactory = streamFactory
            self.reconcileOnStart = reconcileOnStart
        }

        private var isActive: Bool {
            cancellationLock.withLock { !invalidated }
        }

        func start(paths: [String]) {
            queue.async { [self] in
                guard isActive,
                      let created = streamFactory(paths, queue, { [weak self] change in
                          self?.deliver(change)
                      }) else { return }
                guard isActive else {
                    created.stop()
                    return
                }
                stream = created
                guard created.start(), isActive else {
                    stop()
                    return
                }
                // Scanning can finish while native setup is still pending.
                // Reconcile once monitoring is active to cover that entire gap.
                if reconcileOnStart { deliver(.full) }
            }
        }

        func invalidate() {
            cancellationLock.withLock { invalidated = true }
            queue.async { [self] in stop() }
        }

        private func deliver(_ change: Change) {
            guard isActive else { return }
            onChange(change)
        }

        private func stop() {
            stream?.stop()
            stream = nil
        }
    }
}

private final class NativeFolderWatcherStream: FolderWatcherStream, @unchecked Sendable {
    private final class Callback: Sendable {
        let onChange: @Sendable (FolderWatcher.Change) -> Void

        init(_ onChange: @escaping @Sendable (FolderWatcher.Change) -> Void) {
            self.onChange = onChange
        }
    }

    private var stream: FSEventStreamRef?

    init?(paths: [String], queue: DispatchQueue,
          onChange: @escaping @Sendable (FolderWatcher.Change) -> Void) {
        // The native context retains the delivery closure, never the watcher.
        let retained = Unmanaged.passRetained(Callback(onChange))
        var ctx = FSEventStreamContext(
            version: 0,
            info: retained.toOpaque(),
            retain: nil,
            release: { Unmanaged<Callback>.fromOpaque($0!).release() },
            copyDescription: nil
        )

        let flags = FSEventStreamCreateFlags(
            kFSEventStreamCreateFlagUseCFTypes | kFSEventStreamCreateFlagFileEvents
                | kFSEventStreamCreateFlagWatchRoot
        )
        let callback: FSEventStreamCallback = { _, info, count, eventPaths, eventFlags, _ in
            guard let info else { return }
            let paths = unsafeBitCast(eventPaths, to: NSArray.self) as? [String] ?? []
            let flags = Array(UnsafeBufferPointer(start: eventFlags, count: count))
            Unmanaged<Callback>.fromOpaque(info).takeUnretainedValue()
                .onChange(FolderWatcher.Change.events(paths: paths, flags: flags))
        }

        stream = FSEventStreamCreate(
            nil, callback, &ctx,
            paths as CFArray,
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow),
            0.5, // seconds of latency / coalescing
            flags
        )
        guard let stream else {
            // No stream means the `release` callback will never run, so balance
            // the retain above by hand instead of leaking the callback context.
            retained.release()
            return nil
        }
        FSEventStreamSetDispatchQueue(stream, queue)
    }

    func start() -> Bool {
        guard let stream else { return false }
        return FSEventStreamStart(stream)
    }

    func stop() {
        guard let stream else { return }
        FSEventStreamStop(stream)
        FSEventStreamInvalidate(stream)
        FSEventStreamRelease(stream)
        self.stream = nil
    }
}
