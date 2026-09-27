// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Full-bleed overlay that shows one article image large — the "zoomed-in" view
/// raised by clicking a figure (`ImageZoomPresenter.target`). The image is fit to
/// the window at a higher-resolution decode than the inline thumbnail; a click
/// anywhere, the ✕, or Escape dismisses it.
///
/// A top `CursorSurface` owns the pointer across the whole overlay and, crucially,
/// occludes the reader's `NSTextView`s beneath it — otherwise those text views
/// would keep setting their own cursor (an I-beam) through the overlay. It also
/// carries the click-to-dismiss and Escape handling.
extension View {
    /// Wires figure zoom into a reader pane: injects `presenter` into the
    /// environment so figures can raise it, and overlays `ImageLightbox` over the
    /// whole pane while a target is set (fading in and out).
    func imageZoomOverlay(_ presenter: ImageZoomPresenter) -> some View {
        environment(presenter)
            .overlay {
                if let target = presenter.target {
                    ImageLightbox(target: target) { presenter.dismiss() }
                        .transition(.opacity)
                }
            }
            .animation(.easeInOut(duration: 0.2), value: presenter.target)
    }
}

struct ImageLightbox: View {
    let target: ImageZoomPresenter.Target
    let onClose: () -> Void

    @State private var localImage: NSImage?
    @State private var failed = false
    @Environment(\.assetContentGeneration) private var contentGeneration

    var body: some View {
        ZStack {
            Rectangle()
                .fill(.black.opacity(0.92))
                .ignoresSafeArea()

            picture
                .padding(40)

            closeGlyph

            // Top-most: pins the pointer over the overlay, blocks the reader
            // behind it, and dismisses on a click or Escape.
            CursorSurface(cursor: .pointingHand, onClick: onClose, onEscape: onClose)
        }
        .task(id: LoadRequest(targetID: target.id, generation: contentGeneration)) { await loadLocal() }
    }

    @ViewBuilder
    private var picture: some View {
        if let localImage {
            if target.localURL.isLocalGIF {
                displayed(AnimatedLocalGIF(url: target.localURL, fallback: localImage, contentMode: .fit))
            } else {
                displayed(Image(nsImage: localImage).resizable().scaledToFit())
            }
        } else if failed {
            failureView
        } else {
            ProgressView()
                .controlSize(.large)
                .tint(.white)
        }
    }

    private func displayed(_ image: some View) -> some View {
        image
            .accessibilityIdentifier(A11y.Lightbox.image)
            .accessibilityLabel(target.alt.isEmpty ? "Image" : target.alt)
            .accessibilityAddTraits(.isImage)
    }

    private var failureView: some View {
        VStack(spacing: 10) {
            Image(systemName: "photo")
                .font(.system(size: 40))
            Text(target.alt.isEmpty ? "Image unavailable" : target.alt)
        }
        .foregroundStyle(.white.opacity(0.7))
    }

    /// A ✕ affordance pinned top-trailing. Not a control — clicking anywhere
    /// (handled by the `CursorSurface`) dismisses — but it signals how to close.
    private var closeGlyph: some View {
        VStack {
            HStack {
                Spacer()
                Image(systemName: "xmark")
                    .font(.system(size: 14, weight: .semibold))
                    // `.primary` follows the app appearance — black on the light
                    // (whitish) material, white on the dark one — so the glyph
                    // stays legible against the gray circle in either theme.
                    .foregroundStyle(.primary)
                    .frame(width: 30, height: 30)
                    .background(.ultraThinMaterial, in: Circle())
                    .accessibilityIdentifier(A11y.Lightbox.close)
            }
            Spacer()
        }
        .padding(20)
    }

    // ── Loading ───────────────────────────────────────────────────────────────

    private struct LoadRequest: Hashable {
        let targetID: UUID
        let generation: UInt64
    }

    private func loadLocal() async {
        failed = false
        localImage = nil
        let url = target.localURL
        let maxPixel = Self.zoomMaxPixel()
        let decoded = await AssetPreviewDecodeQueue.shared.image(at: url, maxPixel: maxPixel)
        guard !Task.isCancelled else { return }
        if let decoded {
            localImage = decoded.image
        } else {
            failed = true
        }
    }

    /// A generous decode budget for the enlarged view: about twice the longest
    /// screen edge in device pixels, capped so a pathologically large source can't
    /// balloon memory. Comfortably exceeds a fit-to-window display, so the image
    /// stays crisp; smaller sources are never upscaled.
    private static func zoomMaxPixel() -> CGFloat {
        let screen = NSScreen.main
        let size = screen?.frame.size ?? CGSize(width: 1920, height: 1080)
        let backing = screen?.backingScaleFactor ?? 2
        let longestEdge = max(size.width, size.height) * backing
        return min(longestEdge * 2, 4096)
    }
}
