// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Observation
import SwiftUI

extension EnvironmentValues {
    @Entry var assetContentGeneration: UInt64 = 0
}

@MainActor
@Observable
final class BoardScrollState {
    private(set) var isScrolling = false
    private let interactionSourceID = UUID()
    private let interactionGate: InteractionIdleGate

    init(interactionGate: InteractionIdleGate = .shared) {
        self.interactionGate = interactionGate
    }

    deinit {
        let gate = interactionGate
        let sourceID = interactionSourceID
        Task { @MainActor in gate.setScrolling(false, sourceID: sourceID) }
    }

    func setScrolling(_ isScrolling: Bool) {
        guard self.isScrolling != isScrolling else { return }
        self.isScrolling = isScrolling
        interactionGate.setScrolling(isScrolling, sourceID: interactionSourceID)
        PerformanceTrace.scrollPhaseChanged(isScrolling)
    }
}

struct BoardScrollTrackingModifier: ViewModifier {
    let scrollState: BoardScrollState

    func body(content: Content) -> some View {
        content
            .onAppear { scrollState.setScrolling(false) }
            .onScrollPhaseChange { _, phase in
                scrollState.setScrolling(phase.isScrolling)
            }
            .onDisappear { scrollState.setScrolling(false) }
    }
}

struct CardViewportVisibilityModifier: ViewModifier {
    let isEnabled: Bool
    let viewportSize: CGSize
    @Binding var isVisible: Bool

    func body(content: Content) -> some View {
        if isEnabled {
            content.onGeometryChange(for: Bool.self) { proxy in
                VideoCardViewport.containsVisibleArea(
                    of: proxy.frame(in: .scrollView(axis: .vertical)),
                    in: CGRect(origin: .zero, size: viewportSize)
                )
            } action: { visible in
                isVisible = visible
            }
        } else {
            content
        }
    }
}

enum AssetPreviewQuality: Hashable {
    case lightweight
    case display
}

struct AssetPreviewLoadPlan: Equatable {
    static let lightweightMaxPixel: CGFloat = 160
    static let boardDisplayMaxPixel: CGFloat = 1024
    private static let maximumBoardImagePixelCount: CGFloat = 32 * 1024 * 1024
    static let refinementDelay = Duration.milliseconds(120)
    private static let maximumDecodeMaxPixel = CGFloat(Int32.max)
    let initialMaxPixel: CGFloat?
    let refinementMaxPixel: CGFloat?

    init(
        maxPixel: CGFloat,
        loadsProgressively: Bool,
        isVisible: Bool,
        isScrolling: Bool
    ) {
        guard isVisible else {
            initialMaxPixel = nil
            refinementMaxPixel = nil
            return
        }

        let boundedMaxPixel = maxPixel.isFinite
            ? min(max(1, maxPixel.rounded(.up)), Self.maximumDecodeMaxPixel)
            : 1
        guard loadsProgressively else {
            initialMaxPixel = boundedMaxPixel
            refinementMaxPixel = nil
            return
        }

        let lightweightMaxPixel = min(Self.lightweightMaxPixel, boundedMaxPixel)
        initialMaxPixel = lightweightMaxPixel
        refinementMaxPixel = !isScrolling && boundedMaxPixel > lightweightMaxPixel
            ? boundedMaxPixel
            : nil
    }

    static func displayMaxPixel(for size: CGSize, displayScale: CGFloat) -> CGFloat {
        let dimension = max(size.width, size.height)
        guard dimension.isFinite, dimension > 0 else { return 1 }
        let scale = displayScale.isFinite && displayScale > 0 ? displayScale : 1
        let requested = min(boardDisplayMaxPixel, max(1, (dimension * scale).rounded(.up)))
        return [160, 320, 512, 768, 1024].first { $0 >= requested } ?? boardDisplayMaxPixel
    }

    /// A clipped screenshot still needs enough pixels across its full source
    /// width. ImageIO's bound applies to the longest edge of that source, not
    /// the smaller visible crop. Ordinary previews keep their existing tiers.
    static func imageDisplayMaxPixel(
        for size: CGSize, aspectRatio: CGFloat, displayScale: CGFloat
    ) -> CGFloat {
        let ordinaryMaxPixel = displayMaxPixel(for: size, displayScale: displayScale)
        guard size.width.isFinite, size.width > 0,
              aspectRatio.isFinite, aspectRatio > 0,
              size.width / aspectRatio > size.height
        else { return ordinaryMaxPixel }

        let targetWidth = displayMaxPixel(
            for: CGSize(width: size.width, height: 0), displayScale: displayScale
        )
        let requestedLongestEdge = (targetWidth / aspectRatio).rounded(.down)
        // Reserve one short-edge pixel for ImageIO's integer rounding. Solving
        // ratio * edge² + edge <= budget also bounds extremely narrow sources.
        let areaLimitedLongestEdge = (
            2 * maximumBoardImagePixelCount
                / (1 + (1 + 4 * aspectRatio * maximumBoardImagePixelCount).squareRoot())
        ).rounded(.down)
        return max(1, min(requestedLongestEdge, areaLimitedLongestEdge))
    }
}

struct AssetPreviewVariant {
    let image: NSImage
    let decodedForMaxPixel: CGFloat
}

struct AssetPreviewPresentation {
    private(set) var requestURL: URL?
    private(set) var lightweight: AssetPreviewVariant?
    private(set) var display: AssetPreviewVariant?
    private(set) var failed = false

    mutating func reset(for requestURL: URL?) {
        guard self.requestURL != requestURL else { return }
        self = AssetPreviewPresentation(requestURL: requestURL)
    }

    mutating func clear(for requestURL: URL?) {
        self = AssetPreviewPresentation(requestURL: requestURL)
    }

    mutating func publish(
        _ variant: AssetPreviewVariant,
        quality: AssetPreviewQuality,
        for requestURL: URL
    ) {
        reset(for: requestURL)
        switch quality {
        case .lightweight:
            lightweight = variant
        case .display:
            display = variant
        }
        failed = false
        PerformanceTrace.increment("image_publications")
        PerformanceTrace.event("ImagePublished")
    }

    mutating func markFailed(for requestURL: URL?) {
        reset(for: requestURL)
        failed = true
    }

    func variant(
        for _: AssetPreviewQuality,
        requestURL: URL?,
        isVisible: Bool
    ) -> AssetPreviewVariant? {
        guard isVisible, self.requestURL == requestURL else { return nil }
        // Quality describes work we may schedule, not a reason to replace an
        // already displayed bitmap when a scroll gesture begins.
        return display ?? lightweight
    }

    /// The autoclosure deliberately avoids observing scroll phase once this
    /// asset is resolved. Finished cards then leave the phase invalidation graph.
    func refinementMaxPixel(
        maxPixel: CGFloat,
        loadsProgressively: Bool,
        isVisible: Bool,
        requestURL: URL?,
        isScrolling: @autoclosure () -> Bool
    ) -> CGFloat? {
        guard isVisible, loadsProgressively, let requestURL else { return nil }
        let idlePlan = AssetPreviewLoadPlan(
            maxPixel: maxPixel,
            loadsProgressively: true,
            isVisible: true,
            isScrolling: false
        )
        guard let target = idlePlan.refinementMaxPixel,
              !contains(.display, atLeastMaxPixel: target, for: requestURL)
        else { return nil }
        return isScrolling() ? nil : target
    }

    func contains(
        _ quality: AssetPreviewQuality,
        atLeastMaxPixel maxPixel: CGFloat,
        for requestURL: URL
    ) -> Bool {
        guard self.requestURL == requestURL else { return false }
        let variant = switch quality {
        case .lightweight: lightweight
        case .display: display
        }
        return (variant?.decodedForMaxPixel ?? 0) >= maxPixel
    }
}
