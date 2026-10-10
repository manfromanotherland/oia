// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct ProductCard: View {
    let row: ReadingRow
    let libraryURL: URL?
    let cardSize: CGSize
    let displayScale: CGFloat
    let isVisible: Bool
    let scrollState: BoardScrollState

    var body: some View {
        imageCanvas
            .frame(width: cardSize.width, height: cardSize.height)
            .overlay(alignment: .bottomLeading) { titleOverlay }
            .overlay(alignment: .topTrailing) {
                priceBadge.padding(12)
            }
            .clipped()
    }

    private var imageCanvas: some View {
        ZStack {
            if row.previewAsset == nil {
                OiaTheme.textCardSurface
            } else {
                OiaTheme.previewPlaceholderBackground(for: row)

                LocalReadingImage(
                    row: row, libraryURL: libraryURL,
                    fallbackAspectRatio: row.articlePreviewAspectRatio ?? 1,
                    maxPixel: AssetPreviewLoadPlan.displayMaxPixel(
                        for: cardSize, displayScale: displayScale
                    ),
                    contentMode: .fit,
                    loadsProgressively: true,
                    isVisible: isVisible,
                    scrollState: scrollState
                )
                .frame(width: cardSize.width, height: cardSize.height)
            }
        }
    }

    private var titleOverlay: some View {
        VStack(spacing: 0) {
            if row.previewAsset != nil {
                LinearGradient(
                    colors: [.clear, .black.opacity(0.72)],
                    startPoint: .top, endPoint: .bottom
                )
                .frame(height: 48)
                .accessibilityHidden(true)
            }

            Text(row.displayTitle)
                .font(.title2.weight(.semibold))
                .foregroundStyle(row.previewAsset == nil ? Color.primary : .white)
                .lineLimit(3)
                .multilineTextAlignment(.leading)
                .padding(16)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background {
                    if row.previewAsset != nil {
                        // Back every title line with the same minimum scrim,
                        // including longer titles over pale product photos.
                        LinearGradient(
                            colors: [.black.opacity(0.72), .black.opacity(0.9)],
                            startPoint: .top, endPoint: .bottom
                        )
                    }
                }
        }
    }

    private var priceBadge: some View {
        let shape = RoundedRectangle(cornerRadius: 6, style: .continuous)
        return Text(row.productPriceLabel ?? "Product")
            .font(.body.weight(.medium))
            .monospacedDigit()
            .foregroundStyle(.primary)
            .lineLimit(1)
            .minimumScaleFactor(0.75)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(OiaTheme.card, in: shape)
            .overlay(shape.stroke(OiaTheme.border, lineWidth: 0.5))
    }
}
