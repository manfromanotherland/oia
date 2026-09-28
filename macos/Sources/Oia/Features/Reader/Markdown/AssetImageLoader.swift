// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import AVFoundation
import ImageIO

/// Resolves and decodes local visual assets. Article figures and their lightbox
/// share image path resolution, ImageIO downsampling for raster images, and an
/// AppKit fallback for SVGs; video cards reuse the same safe path resolution
/// before deriving a first-frame thumbnail.
enum AssetImageLoader {
    /// Wraps an `NSImage` so a background decode can cross the actor boundary
    /// back to the main actor. Safe because the image is created before it
    /// crosses and is never mutated afterward.
    struct Decoded: @unchecked Sendable {
        let image: NSImage
    }

    /// The on-disk folder that one reading's relative asset links resolve
    /// against: `libraryURL/articles/<prefix>/<id>/`, where `<prefix>` is the
    /// first two characters of the id. This mirrors the library-format fan-out
    /// layout (see `docs/library-format.md`); keeping it in one place means the
    /// reader encodes that layout knowledge exactly once. Returns `nil` when
    /// there is no library or no id.
    static func readingFolderURL(libraryURL: URL?, readingID: String) -> URL? {
        guard let libraryURL, !readingID.isEmpty else { return nil }
        let prefix = String(readingID.prefix(2))
        // Card bodies resolve these URLs repeatedly. Inferring whether each
        // component is a directory makes Foundation query the filesystem.
        return libraryURL
            .appendingPathComponent("articles", isDirectory: true)
            .appendingPathComponent(prefix, isDirectory: true)
            .appendingPathComponent(readingID, isDirectory: true)
    }

    /// Resolve a local asset reference to an on-disk URL under the reading's
    /// folder (`assetBaseURL`, from `readingFolderURL`).
    ///
    /// Only the exact shape core emits is accepted — `assets/<filename>`, one
    /// file inside the reading's own `assets/` folder. Because the library is
    /// synced and externally writable, a crafted link must not be able to read
    /// arbitrary files: absolute paths, `..` traversal, nested or extra path
    /// segments, and any non-`assets/` reference all return `nil`. An image the
    /// extension couldn't capture is still an `http(s)` URL — also not a local
    /// asset — so it returns `nil` too and the reader shows a placeholder rather
    /// than fetching over the network.
    static func localURL(source: String, assetBaseURL: URL?) -> URL? {
        guard let assetBaseURL else { return nil }
        let prefix = "assets/"
        guard source.hasPrefix(prefix) else { return nil }
        let filename = String(source.dropFirst(prefix.count))
        guard isSafeAssetFilename(filename) else { return nil }
        return assetBaseURL
            .appendingPathComponent("assets", isDirectory: true)
            .appendingPathComponent(filename, isDirectory: false)
    }

    /// A safe asset filename is a single path component: non-empty, no `/`
    /// (rules out absolute and nested paths), and not `.`/`..` (rules out
    /// traversal). Core writes `<sha256>.<ext>`, so this never rejects a real
    /// asset while refusing anything that could escape the `assets/` folder.
    private static func isSafeAssetFilename(_ name: String) -> Bool {
        !name.isEmpty && !name.contains("/") && name != "." && name != ".."
    }

    /// Decode `url` into an image whose largest dimension is at most `maxPixel`
    /// device pixels. Raster images use ImageIO so the full-resolution bitmap is
    /// never materialized; SVGs use AppKit for an equivalently bounded raster
    /// because ImageIO does not support them. Smaller raster images are left
    /// as-is (no upscaling). Returns `nil` if the file can't be read or decoded.
    nonisolated static func downsampledImage(at url: URL, maxPixel: CGFloat) -> Decoded? {
        let sourceOptions: [CFString: Any] = [
            kCGImageSourceShouldCache: false
        ]
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: Int(maxPixel.rounded())
        ]
        if let source = CGImageSourceCreateWithURL(url as CFURL, sourceOptions as CFDictionary),
           let cgImage = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
        {
            let size = NSSize(width: cgImage.width, height: cgImage.height)
            return Decoded(image: NSImage(cgImage: cgImage, size: size))
        }

        return svgImage(at: url, maxPixel: maxPixel)
    }

    /// ImageIO can create a source for an SVG but cannot decode an image from
    /// it. AppKit's registered SVG representation can rasterize into the same
    /// requested pixel bound, keeping vector complexity off the scrolling path.
    private nonisolated static func svgImage(at url: URL, maxPixel: CGFloat) -> Decoded? {
        guard url.pathExtension.caseInsensitiveCompare("svg") == .orderedSame,
              let data = try? Data(contentsOf: url),
              let image = NSImage(data: data),
              image.size.width.isFinite,
              image.size.height.isFinite,
              image.size.width > 0,
              image.size.height > 0
        else { return nil }

        let scale = maxPixel / max(image.size.width, image.size.height)
        let pixelWidth = max(1, Int((image.size.width * scale).rounded(.up)))
        let pixelHeight = max(1, Int((image.size.height * scale).rounded(.up)))
        let pixelSize = NSSize(
            width: CGFloat(pixelWidth),
            height: CGFloat(pixelHeight)
        )
        guard let bounded = boundedBitmap(from: image, pixelSize: pixelSize) else { return nil }
        return Decoded(image: NSImage(cgImage: bounded, size: pixelSize))
    }

    private nonisolated static func boundedBitmap(
        from image: NSImage,
        pixelSize: NSSize
    ) -> CGImage? {
        var proposedRect = NSRect(origin: .zero, size: pixelSize)
        guard let rendered = image.cgImage(
            forProposedRect: &proposedRect,
            context: nil,
            hints: [.interpolation: NSImageInterpolation.high]
        ), let context = CGContext(
            data: nil,
            width: Int(pixelSize.width),
            height: Int(pixelSize.height),
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else { return nil }

        context.interpolationQuality = .high
        context.draw(
            rendered,
            in: CGRect(
                x: 0,
                y: 0,
                width: pixelSize.width,
                height: pixelSize.height
            )
        )
        return context.makeImage()
    }

    /// Decode a display-oriented first frame for a locally saved video without
    /// persisting a second copy of the media or reading beyond the safe URL
    /// already resolved by `localURL`.
    nonisolated static func videoThumbnail(at url: URL, maxPixel: CGFloat) async -> Decoded? {
        let generator = AVAssetImageGenerator(asset: AVURLAsset(url: url))
        generator.appliesPreferredTrackTransform = true
        generator.maximumSize = CGSize(width: maxPixel, height: maxPixel)

        do {
            let cgImage = try await generator.image(at: .zero).image
            let size = NSSize(width: cgImage.width, height: cgImage.height)
            return Decoded(image: NSImage(cgImage: cgImage, size: size))
        } catch {
            return nil
        }
    }
}
