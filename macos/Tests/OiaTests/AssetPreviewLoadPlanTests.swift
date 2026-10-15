// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import XCTest

final class AssetPreviewLoadPlanTests: XCTestCase {
    func testScrollingBoardCardRequestsOnlyLightweightPreview() {
        let plan = AssetPreviewLoadPlan(
            maxPixel: 800,
            loadsProgressively: true,
            isVisible: true,
            isScrolling: true
        )

        XCTAssertEqual(plan.initialMaxPixel, 160)
        XCTAssertNil(plan.refinementMaxPixel)
    }

    func testIdleBoardCardStagesDisplayQualityAfterLightweightPreview() {
        let plan = AssetPreviewLoadPlan(
            maxPixel: 800,
            loadsProgressively: true,
            isVisible: true,
            isScrolling: false
        )

        XCTAssertEqual(plan.initialMaxPixel, 160)
        XCTAssertEqual(plan.refinementMaxPixel, 800)
    }

    func testOffscreenOverscanCardDoesNotRequestAnImage() {
        let plan = AssetPreviewLoadPlan(
            maxPixel: 800,
            loadsProgressively: true,
            isVisible: false,
            isScrolling: true
        )

        XCTAssertNil(plan.initialMaxPixel)
        XCTAssertNil(plan.refinementMaxPixel)
    }

    func testDetailImageKeepsImmediateDisplayQualityDecode() {
        let plan = AssetPreviewLoadPlan(
            maxPixel: 1600,
            loadsProgressively: false,
            isVisible: true,
            isScrolling: false
        )

        XCTAssertEqual(plan.initialMaxPixel, 1600)
        XCTAssertNil(plan.refinementMaxPixel)
    }

    func testDisplayQualityRoundsBackingSizeIntoFiniteBoardTiers() {
        XCTAssertEqual(
            AssetPreviewLoadPlan.displayMaxPixel(
                for: CGSize(width: 202.5, height: 337.25), displayScale: 2
            ),
            768
        )
        XCTAssertEqual(
            AssetPreviewLoadPlan.displayMaxPixel(
                for: CGSize(width: 700, height: 900), displayScale: 2
            ),
            1024
        )
        XCTAssertEqual(
            AssetPreviewLoadPlan.displayMaxPixel(
                for: CGSize(width: 300, height: 100), displayScale: .nan
            ),
            320
        )
        XCTAssertEqual(
            AssetPreviewLoadPlan.displayMaxPixel(
                for: CGSize(width: 300, height: 30000), displayScale: 2
            ),
            1024
        )
    }

    func testSmallResizeKeepsTheSameDecodeTier() {
        let before = AssetPreviewLoadPlan.displayMaxPixel(for: CGSize(width: 240, height: 300), displayScale: 2)
        let after = AssetPreviewLoadPlan.displayMaxPixel(for: CGSize(width: 241, height: 301), displayScale: 2)
        XCTAssertEqual(before, after)
        XCTAssertEqual(before, 768)
    }

    func testUnclippedImagesKeepExistingDecodeTiers() {
        for size in [
            CGSize(width: 240, height: 360),
            CGSize(width: 240, height: 240),
            CGSize(width: 640, height: 320)
        ] {
            XCTAssertEqual(
                AssetPreviewLoadPlan.imageDisplayMaxPixel(
                    for: size,
                    aspectRatio: size.width / size.height,
                    displayScale: 2
                ),
                AssetPreviewLoadPlan.displayMaxPixel(for: size, displayScale: 2)
            )
        }
    }

    func testClippedImageDecodePreservesWidthWithinPixelBudget() {
        let ratio: CGFloat = 0.01
        let maxPixel = AssetPreviewLoadPlan.imageDisplayMaxPixel(
            for: CGSize(width: 700, height: 1050), aspectRatio: ratio, displayScale: 2
        )

        XCTAssertGreaterThan(maxPixel, AssetPreviewLoadPlan.boardDisplayMaxPixel)
        XCTAssertLessThanOrEqual(maxPixel * ratio, AssetPreviewLoadPlan.boardDisplayMaxPixel)
        XCTAssertLessThanOrEqual(maxPixel * (maxPixel * ratio).rounded(.up), 32 * 1024 * 1024)
        XCTAssertGreaterThan(maxPixel * ratio, 500)
    }

    func testClippedImageDecodeUsesStableWidthTierAcrossSmallResize() {
        let before = AssetPreviewLoadPlan.imageDisplayMaxPixel(
            for: CGSize(width: 240, height: 360), aspectRatio: 0.1, displayScale: 2
        )
        let after = AssetPreviewLoadPlan.imageDisplayMaxPixel(
            for: CGSize(width: 241, height: 361.5), aspectRatio: 0.1, displayScale: 2
        )

        XCTAssertEqual(before, 5120)
        XCTAssertEqual(after, before)
    }

    @MainActor
    func testDecodedPreviewCacheKeepsRequestedPixelVariantsSeparate() throws {
        let cache = AssetPreviewImageCache(countLimit: 4, totalCostLimit: 10_000_000)
        let url = URL(fileURLWithPath: "/tmp/oia-preview-cache-test.png")
        let lightweight = NSImage(size: NSSize(width: 160, height: 100))
        let display = NSImage(size: NSSize(width: 800, height: 500))
        let lightweightKey = AssetPreviewDecodeKey(
            kind: .image, url: url, maxPixel: 160
        )
        let displayKey = AssetPreviewDecodeKey(
            kind: .image, url: url, maxPixel: 800
        )

        cache.insert(lightweight, for: lightweightKey)
        XCTAssertTrue(try XCTUnwrap(cache.entry(for: lightweightKey)).image === lightweight)
        XCTAssertNil(cache.entry(for: displayKey))

        cache.insert(display, for: displayKey)
        XCTAssertTrue(try XCTUnwrap(cache.entry(for: displayKey)).image === display)
        XCTAssertTrue(try XCTUnwrap(cache.entry(for: lightweightKey)).image === lightweight)
    }

    @MainActor
    func testPresentationDropsEveryVariantWhenAssetIdentityChanges() {
        let firstURL = URL(fileURLWithPath: "/tmp/first-preview.png")
        let secondURL = URL(fileURLWithPath: "/tmp/second-preview.png")
        let image = NSImage(size: NSSize(width: 800, height: 500))
        var presentation = AssetPreviewPresentation()

        presentation.publish(
            AssetPreviewVariant(image: image, decodedForMaxPixel: 800),
            quality: .display,
            for: firstURL
        )
        presentation.reset(for: secondURL)

        XCTAssertEqual(presentation.requestURL, secondURL)
        XCTAssertNil(presentation.lightweight)
        XCTAssertNil(presentation.display)
        XCTAssertFalse(presentation.failed)
    }

    @MainActor
    func testPresentationNeverDowngradesAnAlreadyRefinedImage() throws {
        let url = URL(fileURLWithPath: "/tmp/tiered-preview.png")
        let lightweight = NSImage(size: NSSize(width: 160, height: 100))
        let display = NSImage(size: NSSize(width: 800, height: 500))
        var presentation = AssetPreviewPresentation()

        presentation.publish(
            AssetPreviewVariant(image: lightweight, decodedForMaxPixel: 160),
            quality: .lightweight,
            for: url
        )
        presentation.publish(
            AssetPreviewVariant(image: display, decodedForMaxPixel: 800),
            quality: .display,
            for: url
        )

        XCTAssertTrue(try XCTUnwrap(presentation.variant(
            for: .lightweight,
            requestURL: url,
            isVisible: true
        )).image === display)
        XCTAssertTrue(try XCTUnwrap(presentation.variant(
            for: .display,
            requestURL: url,
            isVisible: true
        )).image === display)
    }

    @MainActor
    func testResolvedCardDoesNotReadScrollStateOrScheduleMoreWork() {
        let url = URL(fileURLWithPath: "/tmp/resolved-preview.png")
        var presentation = AssetPreviewPresentation()
        presentation.publish(
            AssetPreviewVariant(image: NSImage(size: NSSize(width: 800, height: 500)),
                                decodedForMaxPixel: 800),
            quality: .display, for: url
        )
        var scrollReads = 0
        func scrolling() -> Bool {
            scrollReads += 1; return true
        }
        XCTAssertNil(presentation.refinementMaxPixel(
            maxPixel: 800, loadsProgressively: true, isVisible: true,
            requestURL: url, isScrolling: scrolling()
        ))
        XCTAssertEqual(scrollReads, 0)
    }

    @MainActor
    func testUnresolvedCardDefersRefinementUntilIdle() {
        let url = URL(fileURLWithPath: "/tmp/entering-preview.png")
        var presentation = AssetPreviewPresentation()
        presentation.publish(
            AssetPreviewVariant(image: NSImage(size: NSSize(width: 160, height: 100)),
                                decodedForMaxPixel: 160),
            quality: .lightweight, for: url
        )
        XCTAssertNil(presentation.refinementMaxPixel(
            maxPixel: 800, loadsProgressively: true, isVisible: true,
            requestURL: url, isScrolling: true
        ))
        XCTAssertEqual(presentation.refinementMaxPixel(
            maxPixel: 800, loadsProgressively: true, isVisible: true,
            requestURL: url, isScrolling: false
        ), 800)
    }
}
