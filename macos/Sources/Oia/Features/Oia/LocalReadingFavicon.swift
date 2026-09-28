// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// A page favicon rendered from the reading's own local `assets/` folder. It
/// deliberately shares the same resolver, bounded decoder, and cache as card
/// previews; the browser is never consulted while the board is rendering.
struct LocalReadingFavicon: View {
    @Environment(\.assetContentGeneration) private var contentGeneration
    let row: ReadingRow
    let libraryURL: URL?
    var size: CGFloat = 14
    var maxPixel: CGFloat = 64
    var isVisible = true
    var showsFallback = true

    @State private var image: NSImage?

    var body: some View {
        Group {
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
            } else if showsFallback {
                Image(systemName: "globe")
                    .font(.system(size: 10, weight: .regular))
                    .foregroundStyle(fallbackForeground)
            } else {
                Color.clear
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: 3, style: .continuous))
        .accessibilityHidden(true)
        .task(id: loadKey) {
            await load()
        }
    }

    private var loadKey: String {
        "\(libraryURL?.path ?? ""):\(row.id):\(row.faviconAsset ?? "")"
            + ":\(Int(maxPixel)):\(isVisible):\(contentGeneration)"
    }

    private var assetURL: URL? {
        guard let source = row.faviconAsset else { return nil }
        let folder = AssetImageLoader.readingFolderURL(
            libraryURL: libraryURL,
            readingID: row.id
        )
        return AssetImageLoader.localURL(source: source, assetBaseURL: folder)
    }

    private var fallbackForeground: Color {
        OiaTheme.articlePalette(for: row)?.foreground.color ?? .secondary
    }

    @MainActor
    private func load() async {
        image = nil
        guard isVisible, let url = assetURL else { return }
        let loaded = await AssetPreviewDecodeQueue.shared.image(at: url, maxPixel: maxPixel)
        guard !Task.isCancelled, isVisible, let loaded else { return }
        image = loaded.image
    }
}
