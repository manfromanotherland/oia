// SPDX-License-Identifier: GPL-3.0-or-later

import CoreGraphics

extension ReadingRow {
    static let socialPreviewAspectRatio: CGFloat = 1200 / 630
    private static let boardImageClippingThreshold: CGFloat = 2.5
    private static let clippedBoardImageHeightToWidth: CGFloat = 0.75

    /// The intrinsic ratio stays available to full-size detail presentation.
    /// Board image heights apply their own cap without changing this ratio.
    var standaloneMediaAspectRatio: CGFloat? {
        switch kind {
        case .image:
            validMediaAspectRatio ?? 4 / 3
        case .video:
            validMediaAspectRatio ?? 16 / 9
        case .article, .quote:
            nil
        }
    }

    var articlePreviewAspectRatio: CGFloat? {
        guard kind == .article, previewAsset != nil else { return nil }
        return validMediaAspectRatio ?? Self.socialPreviewAspectRatio
    }

    func articlePreviewHeight(for width: CGFloat) -> CGFloat? {
        boardImageHeight(for: width, aspectRatio: articlePreviewAspectRatio)
    }

    /// Product previews use a square or gently portrait canvas so the overlaid
    /// title and price have room even when the saved image is panoramic. The
    /// image itself fits inside this canvas without cropping the product.
    var productCardAspectRatio: CGFloat {
        min(1, max(3 / 4, validMediaAspectRatio ?? 1))
    }

    func productCardHeight(for width: CGFloat) -> CGFloat? {
        height(for: width, aspectRatio: productCardAspectRatio)
    }

    func standaloneMediaHeight(for width: CGFloat) -> CGFloat? {
        if kind == .image {
            return boardImageHeight(for: width, aspectRatio: standaloneMediaAspectRatio)
        }
        return height(for: width, aspectRatio: standaloneMediaAspectRatio)
    }

    var hasLocalSocialPreview: Bool {
        guard let profile = socialPostProfile else { return false }
        return profile.avatarAsset != nil || profile.primaryAttachment != nil
    }

    /// Only exceptionally tall images are clipped; ordinary portrait shapes
    /// keep their full height. Both masonry placement and card rendering use it.
    private func boardImageHeight(for width: CGFloat, aspectRatio: CGFloat?) -> CGFloat? {
        guard let naturalHeight = height(for: width, aspectRatio: aspectRatio) else { return nil }
        guard naturalHeight > width * Self.boardImageClippingThreshold else { return naturalHeight }
        return width * Self.clippedBoardImageHeightToWidth
    }

    private func height(for width: CGFloat, aspectRatio: CGFloat?) -> CGFloat? {
        guard width.isFinite, width > 0,
              let aspectRatio
        else {
            return nil
        }
        return width / aspectRatio
    }

    private var validMediaAspectRatio: CGFloat? {
        guard let mediaAspectRatio,
              mediaAspectRatio.isFinite,
              mediaAspectRatio > 0
        else {
            return nil
        }
        return CGFloat(mediaAspectRatio)
    }
}

extension ReadingSourceAttachment {
    /// The durable dimensions are advisory because files can be externally
    /// edited. Invalid or absent values use a stable media-kind fallback.
    var intrinsicAspectRatio: CGFloat {
        if let width, let height,
           width.isFinite, height.isFinite,
           width > 0, height > 0
        {
            return CGFloat(width / height)
        }
        return mediaKind == .video ? 16 / 9 : 4 / 3
    }

    /// Board cards keep unusually tall or panoramic source media within a
    /// readable post-shaped range. Detail presentation still uses the intrinsic
    /// ratio above.
    var cardAspectRatio: CGFloat {
        min(2, max(3 / 4, intrinsicAspectRatio))
    }
}
