// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Offline-only image rendering for card previews, source-profile attachments,
/// and the image/video overlay. Captured previews are decoded with ImageIO; a
/// local video without a poster derives its thumbnail from the saved movie.
/// Every path stays beneath the reading's own folder and never reaches back to
/// the network.
struct LocalReadingImage: View {
    @Environment(\.boardCardVisibility) private var viewportVisibility
    @Environment(\.assetContentGeneration) private var contentGeneration
    let row: ReadingRow
    let libraryURL: URL?
    var explicitAssetReference: String?
    var explicitAssetIsVideo = false
    var fallbackAspectRatio: CGFloat = 4 / 3
    var maxPixel: CGFloat = 800
    var contentMode: ContentMode = .fit
    var loadsProgressively = false
    var isVisible = true
    var scrollState: BoardScrollState?

    @State private var presentation = AssetPreviewPresentation()
    @State private var validatedRequest: AssetRequest?

    var body: some View {
        Group {
            if let image = presentedImage {
                if let url = assetRequest?.url, url.isLocalGIF {
                    AnimatedLocalGIF(url: url, fallback: image, contentMode: contentMode)
                        .aspectRatio(imageAspectRatio(image), contentMode: contentMode)
                } else {
                    Image(nsImage: image)
                        .resizable()
                        .interpolation(.high)
                        .aspectRatio(imageAspectRatio(image), contentMode: contentMode)
                }
            } else {
                placeholder
                    .aspectRatio(fallbackAspectRatio, contentMode: contentMode)
            }
        }
        .task(id: initialTaskID) {
            await loadInitialVariant()
        }
        .task(id: refinementTaskID) {
            await loadDisplayVariant()
        }
        .onDisappear {
            presentation.clear(for: nil)
            validatedRequest = nil
        }
    }

    private var placeholder: some View {
        ZStack {
            OiaTheme.previewPlaceholderBackground(for: row)
            if showsFailure {
                Image(systemName: "photo.badge.exclamationmark")
                    .font(.system(size: 28, weight: .light))
                    .foregroundStyle(OiaTheme.previewPlaceholderForeground(for: row))
            }
        }
    }

    private var presentedImage: NSImage? {
        guard isVisible else { return nil }
        return presentation.variant(
            for: .display,
            requestURL: assetRequest?.url,
            isVisible: true
        )?.image
    }

    private var showsFailure: Bool {
        isVisible
            && presentation.requestURL == assetRequest?.url
            && presentation.failed
    }

    private var isScrolling: Bool {
        scrollState?.isScrolling ?? false
    }

    private var initialPlan: AssetPreviewLoadPlan {
        AssetPreviewLoadPlan(
            maxPixel: maxPixel,
            loadsProgressively: loadsProgressively,
            isVisible: isVisible || shouldPrefetch,
            isScrolling: true
        )
    }

    private var shouldPrefetch: Bool {
        loadsProgressively && !isVisible && (viewportVisibility?.isNearViewport ?? false)
    }

    private var refinementMaxPixel: CGFloat? {
        presentation.refinementMaxPixel(
            maxPixel: maxPixel,
            loadsProgressively: loadsProgressively,
            isVisible: isVisible,
            requestURL: assetRequest?.url,
            isScrolling: isScrolling
        )
    }

    private var initialTaskID: LoadTaskID {
        LoadTaskID(
            request: assetRequest,
            maxPixel: initialPlan.initialMaxPixel,
            quality: loadsProgressively ? .lightweight : .display,
            prerequisiteIsReady: isVisible
        )
    }

    private var refinementTaskID: LoadTaskID {
        let request = assetRequest
        let refinementMaxPixel = refinementMaxPixel
        let lightweightIsReady = if let request, validatedRequest == request, let refinementMaxPixel {
            presentation.contains(
                .lightweight,
                atLeastMaxPixel: min(
                    AssetPreviewLoadPlan.lightweightMaxPixel,
                    refinementMaxPixel
                ),
                for: request.url
            )
        } else {
            false
        }
        return LoadTaskID(
            request: request,
            maxPixel: refinementMaxPixel,
            quality: .display,
            prerequisiteIsReady: lightweightIsReady
        )
    }
}

private extension LocalReadingImage {
    private func imageAspectRatio(_ image: NSImage) -> CGFloat {
        guard image.size.height > 0 else { return fallbackAspectRatio }
        return image.size.width / image.size.height
    }

    @MainActor
    private func loadInitialVariant() async {
        let request = assetRequest
        guard isVisible else {
            presentation.clear(for: request?.url)
            validatedRequest = nil
            if shouldPrefetch, let request, let initialMaxPixel = initialPlan.initialMaxPixel {
                _ = await decode(request, maxPixel: initialMaxPixel, queue: .prefetch)
            }
            return
        }
        guard let request, let initialMaxPixel = initialPlan.initialMaxPixel else {
            presentation.markFailed(for: request?.url)
            return
        }

        let quality: AssetPreviewQuality = loadsProgressively ? .lightweight : .display
        presentation.reset(for: request.url)
        // Re-entering a warm card should not replay the lightweight stage.
        if loadsProgressively {
            if validatedRequest == request,
               presentation.contains(.display, atLeastMaxPixel: maxPixel, for: request.url)
            {
                return
            }
            if let cached = await AssetPreviewDecodeQueue.shared.cachedPreview(
                at: request.url, maxPixel: maxPixel, kind: request.isVideo ? .video : .image
            ) {
                guard !Task.isCancelled, assetRequest == request, isVisible else { return }
                presentation.publish(
                    AssetPreviewVariant(image: cached.image, decodedForMaxPixel: maxPixel),
                    quality: .display, for: request.url
                )
                validatedRequest = request
                return
            }
            guard !Task.isCancelled, assetRequest == request, isVisible else { return }
        }
        guard validatedRequest != request || !presentation.contains(
            quality,
            atLeastMaxPixel: initialMaxPixel,
            for: request.url
        ) else { return }

        let decoded = await decode(request, maxPixel: initialMaxPixel, queue: .shared)
        completeInitialDecode(
            decoded,
            request: request,
            maxPixel: initialMaxPixel,
            quality: quality
        )
    }

    @MainActor
    private func completeInitialDecode(
        _ decoded: AssetImageLoader.Decoded?,
        request: AssetRequest,
        maxPixel: CGFloat,
        quality: AssetPreviewQuality
    ) {
        guard let decoded else {
            guard !Task.isCancelled, assetRequest == request, isVisible else { return }
            presentation.clear(for: request.url)
            presentation.markFailed(for: request.url)
            validatedRequest = request
            return
        }
        let variant = AssetPreviewVariant(
            image: decoded.image,
            decodedForMaxPixel: maxPixel
        )
        guard !Task.isCancelled, assetRequest == request, isVisible else { return }
        if validatedRequest != request {
            presentation.clear(for: request.url)
        }
        presentation.publish(variant, quality: quality, for: request.url)
        validatedRequest = request
    }

    @MainActor
    private func loadDisplayVariant() async {
        guard refinementTaskID.prerequisiteIsReady,
              let request = assetRequest,
              let refinementMaxPixel
        else { return }

        do {
            try await Task.sleep(for: AssetPreviewLoadPlan.refinementDelay)
        } catch {
            return
        }
        guard !Task.isCancelled else { return }

        let decoded = await decode(
            request, maxPixel: refinementMaxPixel, queue: .refinement
        )
        guard decoded != nil, await AssetPreviewPublicationQueue.shared.waitForTurn() else { return }
        completeDisplayDecode(
            decoded,
            request: request,
            maxPixel: refinementMaxPixel
        )
    }

    @MainActor
    private func completeDisplayDecode(
        _ decoded: AssetImageLoader.Decoded?,
        request: AssetRequest,
        maxPixel: CGFloat
    ) {
        guard let decoded else { return }
        let variant = AssetPreviewVariant(
            image: decoded.image,
            decodedForMaxPixel: maxPixel
        )
        guard !Task.isCancelled,
              assetRequest == request,
              !isScrolling,
              isVisible
        else { return }
        presentation.publish(variant, quality: .display, for: request.url)
    }

    private var assetRequest: AssetRequest? {
        let source: String
        let isVideo: Bool
        if let explicitAssetReference {
            source = explicitAssetReference
            isVideo = explicitAssetIsVideo
        } else if let previewAsset = row.previewAsset {
            source = previewAsset
            isVideo = false
        } else if let videoAsset = row.localVideoAssetReference {
            source = videoAsset
            isVideo = true
        } else {
            return nil
        }

        let baseURL = AssetImageLoader.readingFolderURL(
            libraryURL: libraryURL, readingID: row.id
        )
        guard let url = AssetImageLoader.localURL(source: source, assetBaseURL: baseURL)
        else { return nil }
        return AssetRequest(url: url, isVideo: isVideo, contentGeneration: contentGeneration)
    }

    private func decode(
        _ request: AssetRequest,
        maxPixel: CGFloat,
        queue: AssetPreviewDecodeQueue
    ) async -> AssetImageLoader.Decoded? {
        if request.isVideo {
            return await queue.videoThumbnail(at: request.url, maxPixel: maxPixel)
        }
        return await queue.image(at: request.url, maxPixel: maxPixel)
    }

    private struct AssetRequest: Hashable {
        let url: URL
        let isVideo: Bool
        let contentGeneration: UInt64
    }

    private struct LoadTaskID: Hashable {
        let request: AssetRequest?
        let maxPixel: Int?
        let quality: AssetPreviewQuality
        let prerequisiteIsReady: Bool

        init(
            request: AssetRequest?,
            maxPixel: CGFloat?,
            quality: AssetPreviewQuality,
            prerequisiteIsReady: Bool
        ) {
            self.request = request
            self.maxPixel = maxPixel.map { Int($0.rounded(.up)) }
            self.quality = quality
            self.prerequisiteIsReady = prerequisiteIsReady
        }
    }
}
