// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Renders a single Markdown image natively, with an optional caption drawn from
/// the image's alt text (a "figure"). Local library assets (`assets/<file>`,
/// relative to the reading's `article.md`) load from disk under `assetBaseURL`
/// (the reading's folder). An image
/// the extension couldn't capture at save time is left as a remote `http(s)` URL;
/// the reader shows a labelled placeholder for it and never fetches it over the
/// network. Path resolution and downsampled decoding live in `AssetImageLoader`,
/// shared with the zoom `ImageLightbox`.
///
/// Clicking the picture raises the lightbox (via `ImageZoomPresenter`, injected
/// into the reader's environment) so the reader can zoom into detail the inline
/// thumbnail doesn't hold. When no presenter is present (e.g. previews) the
/// figure stays a plain, non-interactive image.
struct AssetImageView: View {
    let source: String
    let alt: String
    let assetBaseURL: URL?
    let theme: MarkdownTheme
    var highlights: [String] = []
    var onHighlight: (String) -> Void = { _ in }

    @Environment(ImageZoomPresenter.self) private var zoomPresenter: ImageZoomPresenter?
    @Environment(\.displayScale) private var displayScale
    @Environment(\.assetContentGeneration) private var contentGeneration

    @State private var localImage: NSImage?
    @State private var failed = false
    @State private var loadedURL: URL?

    var body: some View {
        VStack(spacing: theme.captionGap) {
            picture
                .frame(maxWidth: .infinity)
                .accessibilityLabel(alt)
            if !alt.isEmpty {
                // A selectable text view (rather than SwiftUI `Text`) so the
                // caption supports the same highlight tint and Highlight/Look Up
                // menu as body text.
                SelectableTextView(attributed: captionAttributed,
                                   highlights: highlights, onHighlight: onHighlight)
                    .frame(maxWidth: .infinity)
            }
        }
        .task(id: loadRequest) { await loadLocal() }
        .onDisappear { localImage = nil }
    }

    /// The alt-text caption as an `NSAttributedString` (centered, secondary,
    /// caption-sized) so it can back a `SelectableTextView`.
    private var captionAttributed: NSAttributedString {
        let paragraph = NSMutableParagraphStyle()
        paragraph.alignment = .center
        paragraph.lineBreakMode = .byWordWrapping
        let font = AppKitInline.makeFont(size: theme.captionSize, weight: .regular,
                                         design: theme.design, bold: false, italic: false)
        return NSAttributedString(string: alt, attributes: [
            .font: font,
            .foregroundColor: NSColor.secondaryLabelColor,
            .paragraphStyle: paragraph
        ])
    }

    @ViewBuilder
    private var picture: some View {
        if let localImage {
            // Cap the display width at the image's own width so a picture narrower
            // than the content column keeps its natural size instead of being
            // upscaled to fill the width. A larger image still scales down to the
            // column, via `scaledToFit` under the surrounding `maxWidth: .infinity`.
            zoomable(
                Group {
                    if let url = loadRequest.url, url.isLocalGIF {
                        AnimatedLocalGIF(url: url, fallback: localImage, contentMode: .fit)
                    } else {
                        Image(nsImage: localImage).resizable().scaledToFit()
                    }
                }
                .frame(maxWidth: localImage.size.width)
                .clipShape(RoundedRectangle(cornerRadius: theme.imageCornerRadius))
            )
        } else if failed {
            placeholder
        } else {
            ProgressView()
                .frame(maxWidth: .infinity, minHeight: 80)
        }
    }

    /// Make a successfully loaded figure clickable: a `CursorSurface` over the
    /// image owns the pointing-hand cursor (reliably, unlike a hover overlay that
    /// loses to the reader's text views) and opens the lightbox on click. When no
    /// presenter is in the environment the image is returned unchanged (no zoom
    /// affordance).
    @ViewBuilder
    private func zoomable(_ image: some View) -> some View {
        if let zoomPresenter {
            image
                .overlay {
                    CursorSurface(cursor: .pointingHand, onClick: {
                        zoomPresenter.present(source: source, alt: alt, assetBaseURL: assetBaseURL)
                    })
                    // The surface must win *hit-testing* to own the cursor and
                    // carry the click, which also makes it occlude the figure in
                    // the accessibility hit test — leaving the figure button "not
                    // hittable" for VoiceOver and XCUITest. Hide it from
                    // accessibility so the figure element beneath is what the hit
                    // test resolves to; the click still lands on the surface.
                    .accessibilityHidden(true)
                }
                .help("Click to zoom")
                .accessibilityAddTraits(.isButton)
                .accessibilityIdentifier(A11y.Reader.figure)
        } else {
            image
        }
    }

    private var placeholder: some View {
        HStack(spacing: 8) {
            Image(systemName: "photo")
            Text(alt.isEmpty ? "Image unavailable" : alt)
        }
        .foregroundStyle(.secondary)
        .font(.callout)
        .frame(maxWidth: .infinity, minHeight: 80)
        .background(.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: theme.imageCornerRadius))
    }

    // ── Resolution ──────────────────────────────────────────────────────────

    private struct LoadRequest: Hashable {
        let url: URL?
        let maxPixel: CGFloat
        let generation: UInt64
    }

    private var loadRequest: LoadRequest {
        LoadRequest(
            url: AssetImageLoader.localURL(source: source, assetBaseURL: assetBaseURL),
            maxPixel: theme.contentMaxWidth * displayScale,
            generation: contentGeneration
        )
    }

    private func loadLocal() async {
        let request = loadRequest
        if loadedURL != request.url {
            localImage = nil
        }
        loadedURL = request.url
        failed = false
        guard let url = request.url else {
            failed = true
            return
        }
        // The reader never lays an image out wider than `contentMaxWidth`, so a
        // thumbnail that many device pixels across is all the inline display
        // needs. Loading at native resolution instead would hold far more memory
        // than the on-screen size warrants, and a long article stacks several
        // images. (The lightbox loads a larger decode on demand for zooming.)
        // Share the bounded decoder with the board so a long article cannot
        // start one original-image decode per figure at the same time.
        let decoded = await AssetPreviewDecodeQueue.shared.image(at: url, maxPixel: request.maxPixel)
        guard !Task.isCancelled else { return }
        localImage = decoded?.image
        failed = decoded == nil
    }
}
