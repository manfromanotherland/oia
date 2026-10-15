// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// A saved image can fit the window or be inspected at one source pixel per
/// display pixel. The latter keeps long screenshots readable and scrollable.
struct OiaImageDetailView: View {
    @Environment(\.displayScale) private var displayScale
    @Environment(\.assetContentGeneration) private var contentGeneration

    let row: ReadingRow
    let libraryURL: URL?

    @State private var pixelSize: CGSize?
    @State private var metadataLoaded = false
    @AppStorage("detailImageScale", store: AppDefaults.store)
    private var preferredImageScale = ImageScale.automatic
    @State private var scrollPosition = ScrollPosition()

    private static let imagePadding: CGFloat = 38
    private static let maximumDecodedPixels: CGFloat = 32_000_000

    var body: some View {
        GeometryReader { geometry in
            if let pixelSize {
                image(pixelSize: pixelSize, viewport: geometry.size)
                    .id(imageRequest)
            } else if metadataLoaded {
                framedImage(maxPixel: 3200)
                    .padding(Self.imagePadding)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ProgressView("Loading image")
                    .controlSize(.small)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Picker("Image Size", selection: imageScaleSelection) {
                    Text("Fit").tag(ImageScale.fit)
                    Text("Original Size").tag(ImageScale.original)
                }
                .pickerStyle(.segmented)
                .fixedSize()
                .disabled(pixelSize == nil)
                .help("Fit the image to the window or view its original pixel size")
                .accessibilityIdentifier(A11y.Detail.imageSize)
            }
        }
        .task(id: imageRequest) {
            await loadMetadata()
        }
    }

    private func image(pixelSize: CGSize, viewport: CGSize) -> some View {
        let size = displaySize(pixelSize: pixelSize, viewport: viewport)
        return ScrollView([.horizontal, .vertical]) {
            framedImage(maxPixel: decodeMaxPixel(pixelSize: pixelSize, displaySize: size))
                .frame(width: size.width, height: size.height)
                .padding(Self.imagePadding)
                .frame(
                    minWidth: viewport.width, minHeight: viewport.height,
                    alignment: size.height + 2 * Self.imagePadding > viewport.height ? .top : .center
                )
        }
        .defaultScrollAnchor(.topLeading)
        .scrollPosition($scrollPosition)
        .onChange(of: size) { _, _ in
            scrollPosition.scrollTo(point: .zero)
        }
        .accessibilityIdentifier(A11y.Detail.imageViewport)
    }

    private var imageScale: ImageScale {
        guard preferredImageScale == .automatic else { return preferredImageScale }
        guard let pixelSize else { return .fit }
        return pixelSize.height > pixelSize.width * 2.5 ? .original : .fit
    }

    private var imageScaleSelection: Binding<ImageScale> {
        Binding(
            get: { imageScale },
            set: { preferredImageScale = $0 }
        )
    }

    private func framedImage(maxPixel: CGFloat) -> some View {
        LocalReadingImage(
            row: row, libraryURL: libraryURL,
            fallbackAspectRatio: row.standaloneMediaAspectRatio ?? 4 / 3,
            maxPixel: maxPixel, contentMode: .fit
        )
        .clipShape(OiaTheme.cardShape)
        .overlay {
            OiaTheme.cardShape
                .stroke(OiaTheme.border, lineWidth: 1)
                .allowsHitTesting(false)
        }
        .accessibilityLabel(row.displayTitle)
    }

    private func displaySize(pixelSize: CGSize, viewport: CGSize) -> CGSize {
        let original = CGSize(
            width: pixelSize.width / max(displayScale, 1),
            height: pixelSize.height / max(displayScale, 1)
        )
        guard imageScale == .fit else { return original }
        let availableWidth = max(1, viewport.width - 2 * Self.imagePadding)
        let availableHeight = max(1, viewport.height - 2 * Self.imagePadding)
        let fitScale = min(1, availableWidth / original.width, availableHeight / original.height)
        return CGSize(width: original.width * fitScale, height: original.height * fitScale)
    }

    private func decodeMaxPixel(pixelSize: CGSize, displaySize: CGSize) -> CGFloat {
        let sourceMax = max(pixelSize.width, pixelSize.height)
        let displayMax = max(displaySize.width, displaySize.height) * max(displayScale, 1)
        let memoryScale = min(1, sqrt(Self.maximumDecodedPixels / (pixelSize.width * pixelSize.height)))
        return max(1, min(sourceMax, displayMax, (sourceMax * memoryScale).rounded(.down)))
    }

    private var imageRequest: ImageRequest? {
        guard let source = row.previewAsset else { return nil }
        let baseURL = AssetImageLoader.readingFolderURL(libraryURL: libraryURL, readingID: row.id)
        guard let url = AssetImageLoader.localURL(source: source, assetBaseURL: baseURL) else { return nil }
        return ImageRequest(url: url, contentGeneration: contentGeneration)
    }

    @MainActor
    private func loadMetadata() async {
        pixelSize = nil
        metadataLoaded = false
        guard let request = imageRequest else {
            metadataLoaded = true
            return
        }
        let metadataTask = Task.detached(priority: .userInitiated) {
            guard !Task.isCancelled else { return CGSize?.none }
            return AssetImageLoader.imagePixelSize(at: request.url)
        }
        let loadedSize = await withTaskCancellationHandler {
            await metadataTask.value
        } onCancel: {
            metadataTask.cancel()
        }
        guard !Task.isCancelled, imageRequest == request else { return }
        pixelSize = loadedSize
        metadataLoaded = true
    }

    private enum ImageScale: String {
        case automatic
        case fit
        case original
    }

    private struct ImageRequest: Hashable {
        let url: URL
        let contentGeneration: UInt64
    }
}
