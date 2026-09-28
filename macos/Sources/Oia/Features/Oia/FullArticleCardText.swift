// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// The content-sized part of a full article card, shared by text-only and
/// image-led cards. Descriptions and reading time arrive from the core's row
/// projection.
struct FullArticleCardText: View {
    let row: ReadingRow
    let libraryURL: URL?
    let isVisible: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: OiaCardTextMetrics.fullArticleSpacing) {
            Text(row.displayTitle)
                .font(Font(OiaCardTextMetrics.fullArticleTitleFont))
                .foregroundStyle(.primary)
                .lineLimit(OiaCardTextMetrics.fullArticleTitleLineLimit)
                .fixedSize(horizontal: false, vertical: true)

            if let description = row.cardDescription, !description.isEmpty {
                Text(description)
                    .font(Font(OiaCardTextMetrics.fullArticleBodyFont))
                    .foregroundStyle(.secondary)
                    .lineSpacing(OiaCardTextMetrics.fullArticleLineSpacing)
                    .lineLimit(OiaCardTextMetrics.fullArticleDescriptionLineLimit)
                    .fixedSize(horizontal: false, vertical: true)
            }

            sourceLine
        }
        .padding(OiaCardTextMetrics.fullArticlePadding)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var sourceLine: some View {
        HStack(spacing: 6) {
            if row.faviconAsset != nil {
                LocalReadingFavicon(
                    row: row,
                    libraryURL: libraryURL,
                    isVisible: isVisible,
                    showsFallback: false
                )
            }

            Text(row.displaySite ?? "Saved locally")
                .lineLimit(1)

            if let readingTime = row.readingTimeLabel {
                Spacer(minLength: 4)
                Text("\(readingTime) read")
                    .fixedSize()
            }
        }
        .font(Font(OiaCardTextMetrics.fullArticleMetadataFont))
        .foregroundStyle(.secondary)
        .frame(height: OiaCardTextMetrics.fullArticleMetadataLineHeight)
    }
}
