// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import ImageIO
import UniformTypeIdentifiers
import XCTest

final class AssetImageMetadataTests: XCTestCase {
    func testSourceDimensionsAreIndependentOfThumbnailResolution() throws {
        let url = try raster(width: 180, height: 1800)
        defer { try? FileManager.default.removeItem(at: url) }

        let thumbnail = try XCTUnwrap(AssetImageLoader.downsampledImage(at: url, maxPixel: 180))
        XCTAssertEqual(thumbnail.image.size, CGSize(width: 18, height: 180))
        XCTAssertEqual(AssetImageLoader.imagePixelSize(at: url), CGSize(width: 180, height: 1800))
    }

    func testRotatedSourceDimensionsMatchDisplayOrientation() throws {
        let url = try raster(width: 120, height: 80, orientation: 6)
        defer { try? FileManager.default.removeItem(at: url) }

        XCTAssertEqual(AssetImageLoader.imagePixelSize(at: url), CGSize(width: 80, height: 120))
        let thumbnail = try XCTUnwrap(AssetImageLoader.downsampledImage(at: url, maxPixel: 120))
        XCTAssertEqual(thumbnail.image.size, CGSize(width: 80, height: 120))
    }

    func testSVGDimensionsUseTheLocalVectorSource() throws {
        let url = URL.temporaryDirectory.appendingPathComponent("oia-size-\(UUID()).svg")
        defer { try? FileManager.default.removeItem(at: url) }
        try Data("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"240\" height=\"120\"></svg>".utf8)
            .write(to: url)

        XCTAssertEqual(AssetImageLoader.imagePixelSize(at: url), CGSize(width: 240, height: 120))
    }

    func testMissingInvalidAndRemoteSourcesHaveNoDimensions() throws {
        let url = URL.temporaryDirectory.appendingPathComponent("oia-size-\(UUID()).png")
        defer { try? FileManager.default.removeItem(at: url) }

        XCTAssertNil(AssetImageLoader.imagePixelSize(at: url))
        try Data("invalid image".utf8).write(to: url)
        XCTAssertNil(AssetImageLoader.imagePixelSize(at: url))
        XCTAssertNil(AssetImageLoader.imagePixelSize(at: try XCTUnwrap(URL(string: "https://example.com/image.png"))))
    }

    private func raster(width: Int, height: Int, orientation: Int = 1) throws -> URL {
        let url = URL.temporaryDirectory.appendingPathComponent("oia-size-\(UUID()).tiff")
        let context = try XCTUnwrap(CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        let destination = try XCTUnwrap(CGImageDestinationCreateWithURL(
            url as CFURL, UTType.tiff.identifier as CFString, 1, nil
        ))
        CGImageDestinationAddImage(destination, try XCTUnwrap(context.makeImage()), [
            kCGImagePropertyOrientation: orientation
        ] as CFDictionary)
        XCTAssertTrue(CGImageDestinationFinalize(destination))
        return url
    }
}
