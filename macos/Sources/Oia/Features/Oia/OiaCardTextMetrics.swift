// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import CoreText

private final class OiaCardFontBundleToken: NSObject {}

struct WidthScopedHeightCache<Key: Hashable> {
    private var activeWidth: Int?
    private var values: [Int: [Key: CGFloat]] = [:]
    private var recentWidths: [Int] = []
    private let maximumWidths: Int
    private let maximumEntriesPerWidth: Int

    init(maximumWidths: Int = 3, maximumEntriesPerWidth: Int = 20_000) {
        self.maximumWidths = max(1, maximumWidths)
        self.maximumEntriesPerWidth = max(1, maximumEntriesPerWidth)
    }

    mutating func value(
        for key: Key,
        width: Int,
        calculate: () -> CGFloat
    ) -> CGFloat {
        if activeWidth != width {
            activeWidth = width
            recentWidths.removeAll { $0 == width }
            recentWidths.append(width)
            if recentWidths.count > maximumWidths {
                values.removeValue(forKey: recentWidths.removeFirst())
            }
        }
        if let cached = values[width]?[key] {
            return cached
        }

        let value = calculate()
        if values[width, default: [:]].count < maximumEntriesPerWidth {
            values[width, default: [:]][key] = value
        }
        return value
    }
}

/// Native text measurements for the fixed card frames supplied to
/// LazyLayoutKit. Measurements retain a bounded set of recent column widths and never
/// read assets or construct offscreen card views.
@MainActor
final class OiaCardTextMetrics {
    static let articleTitleFont: NSFont = {
        let preferred = NSFont.preferredFont(forTextStyle: .headline)
        return NSFont.systemFont(ofSize: preferred.pointSize, weight: .semibold)
    }()

    // The board's full articles use the reader's default Palatino hierarchy.
    // Link and social cards retain their own type and geometry.
    private static let fullArticleBodySize = ReaderFontSize.medium.points
    static let fullArticleTitleFont = NSFont(
        name: ReaderFont.serifFaceName(weight: .bold),
        size: fullArticleBodySize * 1.25
    ) ?? NSFont.systemFont(ofSize: fullArticleBodySize * 1.25, weight: .bold)
    static let fullArticleBodyFont = NSFont(
        name: ReaderFont.serifFaceName(weight: .regular),
        size: fullArticleBodySize
    ) ?? NSFont.systemFont(ofSize: fullArticleBodySize)
    static let fullArticleMetadataFont = NSFont(
        name: ReaderFont.serifFaceName(weight: .regular),
        size: fullArticleBodySize * 0.85
    ) ?? NSFont.systemFont(ofSize: fullArticleBodySize * 0.85)
    static let fullArticleLineSpacing = fullArticleBodySize * ReaderLineHeight.normal.extraLeadingMultiple
    static let fullArticlePadding: CGFloat = 20
    static let fullArticleSpacing: CGFloat = 10
    static let fullArticleTitleLineLimit = 4
    static let fullArticleDescriptionLineLimit = 5
    static let fullArticleMetadataLineHeight = max(
        14,
        ceil(fullArticleMetadataFont.ascender - fullArticleMetadataFont.descender
            + fullArticleMetadataFont.leading)
    )

    private static let extraSmallQuoteFont = makeGeorgiaQuoteFont(ofSize: 16)
    private static let smallQuoteFont = makeGeorgiaQuoteFont(ofSize: 18)
    private static let mediumQuoteFont = makeGeorgiaQuoteFont(ofSize: 20)
    private static let largeQuoteFont = makeGeorgiaQuoteFont(ofSize: 22)
    private static let extraLargeQuoteFont = makeGeorgiaQuoteFont(ofSize: 24)
    private static let quoteMarkBasePointSize: CGFloat = 59
    private static let quoteMarkBaseHeight: CGFloat = 15
    private static let quoteMarkBaseVerticalOffset: CGFloat = 21
    private static let quoteMarkBaseSpacing: CGFloat = 23
    private static let extraSmallQuoteMarkFont = makeQuoteFont(
        ofSize: quoteMarkPointSize(for: .extraSmall),
        opticalSize: 6
    )
    private static let smallQuoteMarkFont = makeQuoteFont(
        ofSize: quoteMarkPointSize(for: .small),
        opticalSize: 6
    )
    private static let mediumQuoteMarkFont = makeQuoteFont(
        ofSize: quoteMarkPointSize(for: .medium),
        opticalSize: 6
    )
    private static let largeQuoteMarkFont = makeQuoteFont(
        ofSize: quoteMarkPointSize(for: .large),
        opticalSize: 6
    )
    private static let extraLargeQuoteMarkFont = makeQuoteFont(
        ofSize: quoteMarkPointSize(for: .extraLarge),
        opticalSize: 6
    )

    static let socialPostFont = NSFont.preferredFont(forTextStyle: .body)
    static let sourceFont = NSFont.preferredFont(forTextStyle: .caption2)
    static let articleFooterPadding: CGFloat = 16
    static let articleFooterSpacing: CGFloat = 8
    static let articleFooterSourceLineHeight = max(14, sourceLineHeight)
    static let quoteHorizontalPadding: CGFloat = 20
    static let quoteVerticalPadding: CGFloat = 20
    static let quoteLineSpacing: CGFloat = 7
    static let quoteLineLimit = 12
    static let socialPostPadding: CGFloat = 16
    static let socialPostSpacing: CGFloat = 12
    static let socialPostHeaderHeight = max(
        34,
        ceil(
            NSFont.preferredFont(forTextStyle: .callout).ascender
                - NSFont.preferredFont(forTextStyle: .callout).descender
                + NSFont.preferredFont(forTextStyle: .callout).leading
        )
            + 1
            + ceil(
                NSFont.preferredFont(forTextStyle: .caption1).ascender
                    - NSFont.preferredFont(forTextStyle: .caption1).descender
                    + NSFont.preferredFont(forTextStyle: .caption1).leading
            )
    )
    static let socialPostLineSpacing: CGFloat = 3
    static let socialPostLineLimit = 10

    private var articleFooterHeights = WidthScopedHeightCache<String>()
    private var fullArticleHeights = WidthScopedHeightCache<FullArticleHeightKey>()
    private var quoteHeights = WidthScopedHeightCache<QuoteHeightKey>()
    private var socialPostHeights = WidthScopedHeightCache<SocialPostHeightKey>()

    func articleFooterHeight(for title: String, width: CGFloat) -> CGFloat {
        let textWidth = max(1, width - Self.articleFooterPadding * 2)
        let halfPointWidth = Int((textWidth * 2).rounded())
        return articleFooterHeights.value(for: title, width: halfPointWidth) {
            let measured = Self.measuredArticleTitleHeight(
                title,
                width: CGFloat(halfPointWidth) / 2
            )
            return Self.articleFooterPadding * 2
                + measured
                + Self.articleFooterSpacing
                + Self.articleFooterSourceLineHeight
        }
    }

    func fullArticleTextHeight(for title: String, description: String?, width: CGFloat) -> CGFloat {
        let textWidth = max(1, width - Self.fullArticlePadding * 2)
        let halfPointWidth = Int((textWidth * 2).rounded())
        let visibleDescription = description.flatMap { $0.isEmpty ? nil : $0 }
        let key = FullArticleHeightKey(title: title, description: visibleDescription)
        return fullArticleHeights.value(for: key, width: halfPointWidth) {
            let measuredWidth = CGFloat(halfPointWidth) / 2
            let titleHeight = Self.measuredTextHeight(
                title, width: measuredWidth, font: Self.fullArticleTitleFont,
                lineLimit: Self.fullArticleTitleLineLimit
            )
            let descriptionHeight = visibleDescription.map {
                Self.measuredTextHeight(
                    $0, width: measuredWidth, font: Self.fullArticleBodyFont,
                    lineSpacing: Self.fullArticleLineSpacing,
                    lineLimit: Self.fullArticleDescriptionLineLimit
                )
            } ?? 0
            return Self.fullArticlePadding * 2
                + titleHeight
                + descriptionHeight
                + Self.fullArticleMetadataLineHeight
                + Self.fullArticleSpacing * (visibleDescription == nil ? 1 : 2)
        }
    }

    func quoteCardHeight(
        for text: String,
        width: CGFloat,
        cardSize: CardSize
    ) -> CGFloat {
        let textWidth = max(1, width - Self.quoteHorizontalPadding * 2)
        let halfPointWidth = Int((textWidth * 2).rounded())
        let key = QuoteHeightKey(text: text, cardSize: cardSize)
        return quoteHeights.value(for: key, width: halfPointWidth) {
            let font = Self.quoteFont(for: cardSize)
            let markHeight = Self.quoteMarkHeight(for: cardSize)
            let markSpacing = Self.quoteMarkSpacing(for: cardSize)
            let measured = Self.measuredQuoteHeight(
                text,
                width: CGFloat(halfPointWidth) / 2,
                font: font,
                cardSize: cardSize
            )
            return ceil(
                Self.quoteVerticalPadding * 2
                    + markHeight * 2
                    + markSpacing * 2
                    + measured
            )
        }
    }

    func socialPostCardHeight(
        for text: String,
        width: CGFloat,
        attachmentAspectRatio: CGFloat?
    ) -> CGFloat {
        let contentWidth = max(1, width - Self.socialPostPadding * 2)
        let halfPointWidth = Int((contentWidth * 2).rounded())
        let key = SocialPostHeightKey(
            text: text,
            attachmentAspectRatio: attachmentAspectRatio
        )
        return socialPostHeights.value(for: key, width: halfPointWidth) {
            let measured = Self.measuredSocialPostHeight(
                text,
                width: CGFloat(halfPointWidth) / 2
            )
            let attachmentHeight = attachmentAspectRatio.map { ratio in
                contentWidth / max(0.01, ratio)
            } ?? 0
            let spacingCount: CGFloat = attachmentAspectRatio == nil ? 1 : 2
            return Self.socialPostPadding * 2
                + Self.socialPostHeaderHeight
                + Self.socialPostSpacing * spacingCount
                + measured
                + attachmentHeight
        }
    }

    private static func measuredQuoteHeight(
        _ text: String,
        width: CGFloat,
        font: NSFont,
        cardSize: CardSize
    ) -> CGFloat {
        measuredTextHeight(
            text,
            width: width,
            font: font,
            lineSpacing: quoteLineSpacing,
            lineLimit: quoteLineLimit,
            renderedLineHeight: quoteRenderedLineHeight(for: cardSize)
        )
    }

    /// SwiftUI's Georgia line boxes at the five card sizes, measured against
    /// the actual quote view. AppKit rounds several of these differently.
    private static func quoteRenderedLineHeight(for cardSize: CardSize) -> CGFloat {
        switch cardSize {
        case .extraSmall: 19
        case .small: 21
        case .medium: 22
        case .large: 25
        case .extraLarge: 27
        }
    }

    private static func makeGeorgiaQuoteFont(ofSize size: CGFloat) -> NSFont {
        NSFont(name: "Georgia-Italic", size: size)
            ?? NSFontManager.shared.convert(
                NSFont.systemFont(ofSize: size), toHaveTrait: .italicFontMask
            )
    }

    private static let newsreaderPostScriptName = "Newsreader16pt-Regular"
    private static let weightAxis = NSNumber(value: UInt32(0x7767_6874))
    private static let opticalSizeAxis = NSNumber(value: UInt32(0x6F70_737A))

    private static func makeQuoteFont(ofSize size: CGFloat, opticalSize: CGFloat) -> NSFont {
        guard let fontURL = Bundle(for: OiaCardFontBundleToken.self).url(
            forResource: "Newsreader-VariableFont_opsz-wght",
            withExtension: "ttf"
        ),
            let descriptors = CTFontManagerCreateFontDescriptorsFromURL(fontURL as CFURL)
            as? [CTFontDescriptor],
            let baseDescriptor = descriptors.first(where: { descriptor in
                (CTFontDescriptorCopyAttribute(descriptor, kCTFontNameAttribute) as? String)
                    == newsreaderPostScriptName
            })
        else {
            assertionFailure("Bundled Newsreader variable font is missing")
            return NSFont.systemFont(ofSize: size, weight: .light)
        }

        let variations: [NSNumber: NSNumber] = [
            weightAxis: NSNumber(value: 300),
            opticalSizeAxis: NSNumber(value: Double(opticalSize))
        ]
        let descriptor = CTFontDescriptorCreateCopyWithAttributes(
            baseDescriptor,
            [kCTFontVariationAttribute: variations] as CFDictionary
        )

        return CTFontCreateWithFontDescriptor(descriptor, size, nil) as NSFont
    }

    private static func measuredArticleTitleHeight(_ text: String, width: CGFloat) -> CGFloat {
        let bounds = (text as NSString).boundingRect(
            with: CGSize(width: width, height: .greatestFiniteMagnitude),
            options: [.usesFontLeading, .usesLineFragmentOrigin],
            attributes: [.font: articleTitleFont]
        )
        let maximumHeight = CGFloat(articleTitleLineLimit) * articleTitleLineHeight
        return min(maximumHeight, max(articleTitleLineHeight, ceil(bounds.height)))
    }

    private static func measuredTextHeight(
        _ text: String,
        width: CGFloat,
        font: NSFont,
        lineSpacing: CGFloat = 0,
        lineLimit: Int,
        renderedLineHeight: CGFloat? = nil
    ) -> CGFloat {
        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.lineSpacing = lineSpacing
        let bounds = (text as NSString).boundingRect(
            with: CGSize(width: width, height: .greatestFiniteMagnitude),
            options: [.usesFontLeading, .usesLineFragmentOrigin],
            attributes: [.font: font, .paragraphStyle: paragraphStyle]
        )
        // NSString's layout rectangle includes more vertical font padding than
        // SwiftUI Text. Use it to count wraps, then size those lines using the
        // Palatino line box SwiftUI actually renders.
        let nativeLineHeight = ("Hg" as NSString).boundingRect(
            with: CGSize(width: width, height: .greatestFiniteMagnitude),
            options: [.usesFontLeading, .usesLineFragmentOrigin],
            attributes: [.font: font, .paragraphStyle: paragraphStyle]
        ).height
        let nativeStep = nativeLineHeight + lineSpacing
        let measuredLines = 1 + Int(max(0, (bounds.height - nativeLineHeight) / nativeStep).rounded())
        let visibleLines = min(lineLimit, measuredLines)
        let lineHeight = renderedLineHeight ?? (font.ascender - font.descender).rounded()
        return ceil(CGFloat(visibleLines) * lineHeight
            + CGFloat(visibleLines - 1) * lineSpacing)
    }

    private static func measuredSocialPostHeight(_ text: String, width: CGFloat) -> CGFloat {
        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.lineSpacing = socialPostLineSpacing
        let bounds = (text as NSString).boundingRect(
            with: CGSize(width: width, height: .greatestFiniteMagnitude),
            options: [.usesFontLeading, .usesLineFragmentOrigin],
            attributes: [
                .font: socialPostFont,
                .paragraphStyle: paragraphStyle
            ]
        )
        let maximumHeight = CGFloat(socialPostLineLimit) * socialPostLineHeight
            + CGFloat(socialPostLineLimit - 1) * socialPostLineSpacing
        return min(maximumHeight, max(socialPostLineHeight, ceil(bounds.height)))
    }

    private static let articleTitleLineLimit = 3
    private static let articleTitleLineHeight = ceil(
        articleTitleFont.ascender - articleTitleFont.descender + articleTitleFont.leading
    )
    private static let sourceLineHeight = ceil(
        sourceFont.ascender - sourceFont.descender + sourceFont.leading
    )
    private static let socialPostLineHeight = ceil(
        socialPostFont.ascender - socialPostFont.descender + socialPostFont.leading
    )

    private struct SocialPostHeightKey: Hashable {
        let text: String
        let attachmentAspectRatio: CGFloat?
    }

    private struct QuoteHeightKey: Hashable {
        let text: String
        let cardSize: CardSize
    }

    private struct FullArticleHeightKey: Hashable {
        let title: String
        let description: String?
    }
}

extension OiaCardTextMetrics {
    static func quotePointSize(for cardSize: CardSize) -> CGFloat {
        switch cardSize {
        case .extraSmall: 16
        case .small: 18
        case .medium: 20
        case .large: 22
        case .extraLarge: 24
        }
    }

    static func quoteMarkPointSize(for cardSize: CardSize) -> CGFloat {
        quoteMarkBasePointSize * quoteScale(for: cardSize)
    }

    static func quoteMarkHeight(for cardSize: CardSize) -> CGFloat {
        quoteMarkBaseHeight * quoteScale(for: cardSize)
    }

    static func quoteMarkVerticalOffset(for cardSize: CardSize) -> CGFloat {
        quoteMarkBaseVerticalOffset * quoteScale(for: cardSize)
    }

    static func quoteMarkSpacing(for cardSize: CardSize) -> CGFloat {
        quoteMarkBaseSpacing * quoteScale(for: cardSize)
    }

    static func quoteFont(for cardSize: CardSize) -> NSFont {
        switch cardSize {
        case .extraSmall: extraSmallQuoteFont
        case .small: smallQuoteFont
        case .medium: mediumQuoteFont
        case .large: largeQuoteFont
        case .extraLarge: extraLargeQuoteFont
        }
    }

    static func quoteMarkFont(for cardSize: CardSize) -> NSFont {
        switch cardSize {
        case .extraSmall: extraSmallQuoteMarkFont
        case .small: smallQuoteMarkFont
        case .medium: mediumQuoteMarkFont
        case .large: largeQuoteMarkFont
        case .extraLarge: extraLargeQuoteMarkFont
        }
    }

    private static func quoteScale(for cardSize: CardSize) -> CGFloat {
        quotePointSize(for: cardSize) / quotePointSize(for: .extraLarge)
    }
}
