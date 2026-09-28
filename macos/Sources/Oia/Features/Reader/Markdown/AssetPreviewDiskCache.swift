// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import CryptoKit
import Darwin
import ImageIO
import UniformTypeIdentifiers

/// Includes change time as well as modification time: external writers can preserve mtime.
struct AssetPreviewSourceFingerprint: Hashable, Sendable {
    let value: String

    static func read(at url: URL) -> Self? {
        var info = stat()
        let result = url.withUnsafeFileSystemRepresentation { path in
            guard let path else { return Int32(-1) }
            return fstatat(AT_FDCWD, path, &info, 0)
        }
        guard result == 0, (info.st_mode & S_IFMT) == S_IFREG else { return nil }
        return Self(value: "\(info.st_dev):\(info.st_ino):\(info.st_size):"
            + "\(info.st_mtimespec.tv_sec):\(info.st_mtimespec.tv_nsec):"
            + "\(info.st_ctimespec.tv_sec):\(info.st_ctimespec.tv_nsec)")
    }
}

struct AssetPreviewDiskKey: Sendable {
    let filename: String
    let sourceURL: URL
    let fingerprint: AssetPreviewSourceFingerprint
    let maxPixel: Int

    init(_ key: AssetPreviewDecodeKey, fingerprint: AssetPreviewSourceFingerprint) {
        sourceURL = URL(fileURLWithPath: key.path)
        self.fingerprint = fingerprint
        maxPixel = key.maxPixel
        let identity = "v1:\(key.kind):\(key.path):\(fingerprint.value):\(key.maxPixel)"
        filename = SHA256.hash(data: Data(identity.utf8))
            .map { String(format: "%02x", $0) }.joined() + ".png"
    }
}

/// Disposable per-device raster previews. All methods that touch files run off-main.
/// Entries are self-identifying, atomically replaced, and evicted by byte cost.
final class AssetPreviewDiskCache: @unchecked Sendable {
    static let shared = AssetPreviewDiskCache(rootURL: defaultRootURL())

    let rootURL: URL
    private let byteLimit: Int
    private let interactionGate: InteractionIdleGate?
    private let lock = NSLock()
    private var entries: [String: Entry]?
    private var totalBytes = 0
    private var pendingStore: PendingStore?
    private var isDraining = false

    private struct Entry {
        let bytes: Int
        var lastAccess: Date
    }

    private struct PendingStore {
        let decoded: AssetImageLoader.Decoded
        let key: AssetPreviewDiskKey
    }

    init(
        rootURL: URL,
        byteLimit: Int = 512 * 1024 * 1024,
        interactionGate: InteractionIdleGate? = nil
    ) {
        self.rootURL = rootURL
        self.byteLimit = max(0, byteLimit)
        self.interactionGate = interactionGate
    }

    func image(for key: AssetPreviewDiskKey) -> AssetImageLoader.Decoded? {
        guard key.maxPixel <= 1024,
              AssetPreviewSourceFingerprint.read(at: key.sourceURL) == key.fingerprint else { return nil }
        lock.lock()
        loadEntriesIfNeeded()
        let exists = entries?[key.filename] != nil
        lock.unlock()
        guard exists else { return nil }
        let file = rootURL.appendingPathComponent(key.filename)
        guard let decoded = AssetImageLoader.downsampledImage(at: file, maxPixel: CGFloat(key.maxPixel)) else {
            remove(key.filename)
            return nil
        }
        lock.lock()
        loadEntriesIfNeeded()
        entries?[key.filename]?.lastAccess = Date()
        lock.unlock()
        PerformanceTrace.increment("preview_disk_hits")
        return decoded
    }

    /// Keep the newest pending preview while scrolling. The drain may hold one
    /// more bitmap while encoding, so retained work stays bounded at two.
    /// Persistence never delays publication.
    func scheduleStore(_ decoded: AssetImageLoader.Decoded, for key: AssetPreviewDiskKey) {
        guard key.maxPixel <= 1024, byteLimit > 0 else { return }
        lock.lock()
        pendingStore = PendingStore(decoded: decoded, key: key)
        let shouldStart = !isDraining
        if shouldStart { isDraining = true }
        lock.unlock()

        if shouldStart {
            Task.detached(priority: .utility) { [self] in
                await drainPendingStores()
            }
        }
    }

    private func drainPendingStores() async {
        let gate: InteractionIdleGate
        if let interactionGate {
            gate = interactionGate
        } else {
            gate = await InteractionIdleGate.shared
        }

        while true {
            // Check before every encode, including work queued during a prior
            // encode. A new gesture must not release the rest of the queue.
            do {
                try await gate.waitUntilIdle()
            } catch {
                stopDraining()
                return
            }

            guard let pending = takePendingStore() else { return }
            store(pending.decoded, for: pending.key)
        }
    }

    private func takePendingStore() -> PendingStore? {
        lock.lock()
        defer { lock.unlock() }
        guard let pending = pendingStore else {
            isDraining = false
            return nil
        }
        pendingStore = nil
        return pending
    }

    private func stopDraining() {
        lock.lock()
        defer { lock.unlock() }
        pendingStore = nil
        isDraining = false
    }

    func store(_ decoded: AssetImageLoader.Decoded, for key: AssetPreviewDiskKey) {
        guard key.maxPixel <= 1024, byteLimit > 0,
              AssetPreviewSourceFingerprint.read(at: key.sourceURL) == key.fingerprint,
              let data = pngData(decoded.image), data.count <= byteLimit,
              AssetPreviewSourceFingerprint.read(at: key.sourceURL) == key.fingerprint else { return }
        lock.lock()
        defer { lock.unlock() }
        loadEntriesIfNeeded()
        do {
            try FileManager.default.createDirectory(at: rootURL, withIntermediateDirectories: true)
            try data.write(to: rootURL.appendingPathComponent(key.filename), options: .atomic)
            totalBytes -= entries?[key.filename]?.bytes ?? 0
            entries?[key.filename] = Entry(bytes: data.count, lastAccess: Date())
            totalBytes += data.count
            evictToBudget()
        } catch {
            // A disposable cache failure never makes a valid original unavailable.
        }
    }

    func storedByteCount() -> Int {
        lock.lock()
        defer { lock.unlock() }
        loadEntriesIfNeeded()
        return totalBytes
    }

    func hasScheduledStoreWork() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return isDraining
    }

    private func remove(_ filename: String) {
        lock.lock()
        defer { lock.unlock() }
        loadEntriesIfNeeded()
        totalBytes -= entries?.removeValue(forKey: filename)?.bytes ?? 0
        try? FileManager.default.removeItem(at: rootURL.appendingPathComponent(filename))
    }

    private func loadEntriesIfNeeded() {
        guard entries == nil else { return }
        entries = [:]
        let files = (try? FileManager.default.contentsOfDirectory(
            at: rootURL, includingPropertiesForKeys: [.fileSizeKey, .contentModificationDateKey],
            options: [.skipsHiddenFiles]
        )) ?? []
        for file in files where file.pathExtension == "png" {
            guard let values = try? file.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey]),
                  let bytes = values.fileSize else { continue }
            entries?[file.lastPathComponent] = Entry(
                bytes: bytes, lastAccess: values.contentModificationDate ?? .distantPast
            )
            totalBytes += bytes
        }
        evictToBudget()
    }

    private func evictToBudget() {
        guard totalBytes > byteLimit, let entries else { return }
        // Reclaim a little headroom so a full cache does not sort thousands of
        // entries again for every subsequent thumbnail write.
        let target = byteLimit - byteLimit / 10
        for (filename, entry) in entries.sorted(by: { $0.value.lastAccess < $1.value.lastAccess }) {
            guard totalBytes > target else { break }
            let url = rootURL.appendingPathComponent(filename)
            do {
                try FileManager.default.removeItem(at: url)
            } catch {
                guard !FileManager.default.fileExists(atPath: url.path) else { continue }
            }
            self.entries?.removeValue(forKey: filename)
            totalBytes -= entry.bytes
        }
    }

    private func pngData(_ image: NSImage) -> Data? {
        var rect = NSRect(origin: .zero, size: image.size)
        guard let cgImage = image.cgImage(forProposedRect: &rect, context: nil, hints: nil) else { return nil }
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil)
        else { return nil }
        CGImageDestinationAddImage(destination, cgImage, nil)
        guard CGImageDestinationFinalize(destination) else { return nil }
        return data as Data
    }

    private static func defaultRootURL() -> URL {
        if TestHooks.isIsolatedRun {
            if let path = ProcessInfo.processInfo.environment["OIA_PERF_CACHE"], !path.isEmpty {
                return URL(fileURLWithPath: path, isDirectory: true)
            }
            if let path = TestHooks.dbPath {
                return URL(fileURLWithPath: path).deletingLastPathComponent()
                    .appendingPathComponent("previews", isDirectory: true)
            }
        }
        return FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("is.edmundo.cuttings/previews-v1", isDirectory: true)
    }
}
