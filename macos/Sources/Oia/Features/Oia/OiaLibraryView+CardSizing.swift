// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

extension OiaLibraryView {
    func estimatedCardHeight(_ row: ReadingRow, width: CGFloat) -> CGFloat {
        switch row.kind {
        case .image, .video:
            return row.standaloneMediaHeight(for: width) ?? width
        case .quote:
            let text = row.excerpt.flatMap { $0.isEmpty ? nil : $0 } ?? row.displayTitle
            return cardTextMetrics.quoteCardHeight(
                for: text,
                width: width,
                cardSize: cardSize
            )
        case .article:
            if let profile = row.socialPostProfile {
                return cardTextMetrics.socialPostCardHeight(
                    for: row.socialPostText,
                    width: width,
                    attachmentAspectRatio: profile.primaryAttachment?.cardAspectRatio
                )
            }
            if row.isFullArticle {
                let textHeight = cardTextMetrics.fullArticleTextHeight(
                    for: row.displayTitle,
                    description: row.cardDescription,
                    width: width
                )
                return textHeight + (row.articlePreviewHeight(for: width) ?? 0)
            }
            if row.previewAsset != nil {
                let previewHeight = row.articlePreviewHeight(for: width)
                    ?? width / ReadingRow.socialPreviewAspectRatio
                return previewHeight + cardTextMetrics.articleFooterHeight(
                    for: row.displayTitle,
                    width: width
                )
            }
            let titleLines = min(5, max(1, Int(ceil(Double(row.displayTitle.count) / 24))))
            let excerptLines = min(6, max(0, Int(ceil(Double(row.excerpt?.count ?? 0) / 34))))
            return 32 + CGFloat(titleLines * 29 + excerptLines * 20) + 32
        }
    }
}
