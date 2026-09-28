// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import XCTest

final class AssetPreviewDiskCacheTests: XCTestCase {
    @MainActor
    func testScheduledPreviewWaitsForIdleAndPersistsTheLatestScrollCandidate() async throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let gate = InteractionIdleGate(idleGrace: .zero)
        let sourceID = UUID()
        gate.setScrolling(true, sourceID: sourceID)
        let cacheRoot = root.appendingPathComponent("cache")
        let cache = AssetPreviewDiskCache(rootURL: cacheRoot, interactionGate: gate)
        var keys: [AssetPreviewDiskKey] = []

        for index in 0 ..< 3 {
            let source = root.appendingPathComponent("source-\(index).png")
            try rasterData(color: .systemPink).write(to: source)
            let key = try diskKey(source)
            let decoded = try XCTUnwrap(AssetImageLoader.downsampledImage(at: source, maxPixel: 32))
            keys.append(key)
            cache.scheduleStore(decoded, for: key)
        }

        try await Task.sleep(for: .milliseconds(200))
        XCTAssertTrue(cache.hasScheduledStoreWork())
        for key in keys {
            let file = cacheRoot.appendingPathComponent(key.filename)
            XCTAssertFalse(FileManager.default.fileExists(atPath: file.path))
        }

        gate.setScrolling(false, sourceID: sourceID)
        try await waitForScheduledStores(cache)
        let latestFile = cacheRoot.appendingPathComponent(try XCTUnwrap(keys.last).filename)
        XCTAssertTrue(FileManager.default.fileExists(atPath: latestFile.path))
        XCTAssertGreaterThan(cache.storedByteCount(), 0)
        XCTAssertNotNil(AssetPreviewDiskCache(rootURL: cacheRoot).image(for: try XCTUnwrap(keys.last)))
        for key in keys.dropLast() {
            let file = cacheRoot.appendingPathComponent(key.filename)
            XCTAssertFalse(FileManager.default.fileExists(atPath: file.path))
        }
    }

    @MainActor
    func testScheduledPreviewRejectsSourceReplacedDuringScroll() async throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let gate = InteractionIdleGate(idleGrace: .zero)
        let sourceID = UUID()
        gate.setScrolling(true, sourceID: sourceID)
        let source = root.appendingPathComponent("source.png")
        try rasterData(color: .systemPink).write(to: source)
        let key = try diskKey(source)
        let cacheRoot = root.appendingPathComponent("cache")
        let cache = AssetPreviewDiskCache(rootURL: cacheRoot, interactionGate: gate)
        let decoded = try XCTUnwrap(AssetImageLoader.downsampledImage(at: source, maxPixel: 32))
        cache.scheduleStore(decoded, for: key)

        try await Task.sleep(for: .milliseconds(100))
        try rasterData(color: .systemBlue).write(to: source, options: .atomic)
        gate.setScrolling(false, sourceID: sourceID)
        try await waitForScheduledStores(cache)
        let oldFile = cacheRoot.appendingPathComponent(key.filename)
        XCTAssertFalse(FileManager.default.fileExists(atPath: oldFile.path))
    }

    func testPersistentPreviewSurvivesCacheRecreationButNotSourceReplacement() throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("source.png")
        try rasterData(color: .systemPink).write(to: source)
        let key = try diskKey(source)
        let decoded = try XCTUnwrap(AssetImageLoader.downsampledImage(at: source, maxPixel: 32))
        let cacheRoot = root.appendingPathComponent("cache")
        AssetPreviewDiskCache(rootURL: cacheRoot).store(decoded, for: key)
        let recreated = AssetPreviewDiskCache(rootURL: cacheRoot)
        XCTAssertNotNil(recreated.image(for: key))

        let previousDate = try XCTUnwrap(
            source.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate
        )
        try rasterData(color: .systemBlue).write(to: source, options: .atomic)
        try FileManager.default.setAttributes([.modificationDate: previousDate], ofItemAtPath: source.path)
        XCTAssertNotEqual(try diskKey(source).filename, key.filename)
        XCTAssertNil(recreated.image(for: key), "An external writer can replace bytes while preserving mtime")
        XCTAssertNil(try recreated.image(for: diskKey(source)))
    }

    func testFingerprintDetectsInPlaceSameSizeRewriteWithPreservedModificationDate() throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("source")
        try Data("aaaa".utf8).write(to: source)
        let before = try XCTUnwrap(AssetPreviewSourceFingerprint.read(at: source))
        let date = try XCTUnwrap(source.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate)
        let handle = try FileHandle(forWritingTo: source)
        try handle.write(contentsOf: Data("bbbb".utf8))
        try handle.close()
        try FileManager.default.setAttributes([.modificationDate: date], ofItemAtPath: source.path)
        XCTAssertNotEqual(AssetPreviewSourceFingerprint.read(at: source), before)
    }

    func testCorruptPreviewIsDiscardedAndByteAccountingRecovers() throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("source.png")
        try rasterData(color: .systemPink).write(to: source)
        let key = try diskKey(source)
        let cacheRoot = root.appendingPathComponent("cache")
        let cache = AssetPreviewDiskCache(rootURL: cacheRoot)
        try cache.store(XCTUnwrap(AssetImageLoader.downsampledImage(at: source, maxPixel: 32)), for: key)
        try Data("corrupt".utf8).write(to: cacheRoot.appendingPathComponent(key.filename))
        XCTAssertNil(cache.image(for: key))
        XCTAssertEqual(cache.storedByteCount(), 0)
        XCTAssertFalse(FileManager.default.fileExists(atPath: cacheRoot.appendingPathComponent(key.filename).path))
    }

    func testDiskBudgetIsEnforcedAcrossWritesAndRecreation() throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("source.png")
        try rasterData(color: .systemPink).write(to: source)
        let decoded = try XCTUnwrap(AssetImageLoader.downsampledImage(at: source, maxPixel: 32))
        let cacheRoot = root.appendingPathComponent("cache")
        let cache = AssetPreviewDiskCache(rootURL: cacheRoot, byteLimit: 1024)
        for size in 16 ... 40 {
            try cache.store(decoded, for: diskKey(source, maxPixel: size))
            XCTAssertLessThanOrEqual(cache.storedByteCount(), 1024)
        }
        XCTAssertGreaterThan(cache.storedByteCount(), 0)
        let smaller = AssetPreviewDiskCache(rootURL: cacheRoot, byteLimit: 512)
        XCTAssertLessThanOrEqual(smaller.storedByteCount(), 512)
        let files = try FileManager.default.contentsOfDirectory(
            at: cacheRoot, includingPropertiesForKeys: [.fileSizeKey]
        )
        let bytes = try files.reduce(0) { try $0 + ($1.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0) }
        XCTAssertLessThanOrEqual(bytes, 512)
    }

    func testWarmOnlyLookupNeverDecodesAnOriginalAndRejectsDeletedSources() async throws {
        let root = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("source.png")
        try rasterData(color: .systemPink).write(to: source)
        let queue = AssetPreviewDecodeQueue(
            limit: 1, diskCache: AssetPreviewDiskCache(rootURL: root.appendingPathComponent("cache"))
        )
        let coldLookup = await queue.cachedPreview(at: source, maxPixel: 32, kind: .image)
        XCTAssertNil(coldLookup)
        let initial = await queue.image(at: source, maxPixel: 32)
        XCTAssertNotNil(initial)
        let warmLookup = await queue.cachedPreview(at: source, maxPixel: 32, kind: .image)
        XCTAssertNotNil(warmLookup)
        try FileManager.default.removeItem(at: source)
        let removedLookup = await queue.cachedPreview(at: source, maxPixel: 32, kind: .image)
        XCTAssertNil(removedLookup)
    }

    func testDetailMemoryPressureDoesNotEvictBoardOrLightweightVariants() {
        let cache = AssetPreviewImageCache(totalCostLimit: 8 * 1024 * 1024)
        let source = URL(fileURLWithPath: "/fixture/source.png")
        let tiny = AssetPreviewDecodeKey(kind: .image, url: source, maxPixel: 160)
        let board = AssetPreviewDecodeKey(kind: .image, url: source, maxPixel: 512)
        cache.insert(NSImage(size: NSSize(width: 160, height: 160)), for: tiny)
        cache.insert(NSImage(size: NSSize(width: 512, height: 512)), for: board)
        for index in 0 ..< 10 {
            let detail = AssetPreviewDecodeKey(
                kind: .image, url: URL(fileURLWithPath: "/fixture/\(index).png"), maxPixel: 3200
            )
            cache.insert(NSImage(size: NSSize(width: 700, height: 700)), for: detail)
        }
        XCTAssertNotNil(cache.entry(for: tiny))
        XCTAssertNotNil(cache.entry(for: board))
        XCTAssertLessThanOrEqual(cache.storedByteCount(), 8 * 1024 * 1024)
    }

    private func diskKey(_ url: URL, maxPixel: Int = 32) throws -> AssetPreviewDiskKey {
        try AssetPreviewDiskKey(
            AssetPreviewDecodeKey(kind: .image, url: url, maxPixel: CGFloat(maxPixel)),
            fingerprint: XCTUnwrap(AssetPreviewSourceFingerprint.read(at: url))
        )
    }

    @MainActor
    private func waitForScheduledStores(_ cache: AssetPreviewDiskCache) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(3))
        while cache.hasScheduledStoreWork(), ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertFalse(cache.hasScheduledStoreWork(), "Scheduled preview work should finish after idle")
    }

    private func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("oia-preview-cache-\(UUID())")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private func rasterData(color: NSColor) throws -> Data {
        let context = try XCTUnwrap(CGContext(
            data: nil, width: 32, height: 32, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        context.setFillColor(color.cgColor)
        context.fill(CGRect(x: 0, y: 0, width: 32, height: 32))
        return try XCTUnwrap(NSBitmapImageRep(cgImage: XCTUnwrap(context.makeImage()))
            .representation(using: .png, properties: [:]))
    }
}
