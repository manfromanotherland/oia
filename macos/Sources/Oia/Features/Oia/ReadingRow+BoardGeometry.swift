// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Only the durable fields that affect estimatedCardHeight participate in the
/// board's geometry revision. Tags, palette, favicons, and other content updates
/// still reach cells without remeasuring every card in the library.
enum BoardCardGeometryKey: Hashable {
    case media(Double)
    case quote(String)
    case social(String, Double?)
    case fullArticle(String, String?, Double?)
    case previewArticle(String, Double)
    case textArticle(String, String?)
}

extension ReadingRow {
    var boardGeometryKey: BoardCardGeometryKey {
        switch kind {
        case .image, .video:
            .media(Double(standaloneMediaAspectRatio ?? 1))
        case .quote:
            .quote(excerpt.flatMap { $0.isEmpty ? nil : $0 } ?? displayTitle)
        case .article:
            if let profile = socialPostProfile {
                .social(socialPostText, profile.primaryAttachment.map { Double($0.cardAspectRatio) })
            } else if isFullArticle {
                .fullArticle(
                    displayTitle,
                    cardDescription,
                    articlePreviewAspectRatio.map(Double.init)
                )
            } else if previewAsset != nil {
                .previewArticle(displayTitle, Double(articlePreviewAspectRatio ?? Self.socialPreviewAspectRatio))
            } else {
                .textArticle(displayTitle, excerpt)
            }
        }
    }
}
