// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import XCTest

@MainActor
final class OiaCardTextMetricsTests: XCTestCase {
    func testWidthScopedCacheReusesRecentWidthsAndEvictsTheOldest() {
        var cache = WidthScopedHeightCache<String>()
        var calculations = 0

        func height(at width: Int) -> CGFloat {
            cache.value(for: "same text", width: width) {
                calculations += 1
                return CGFloat(calculations)
            }
        }

        XCTAssertEqual(height(at: 440), 1)
        XCTAssertEqual(height(at: 440), 1)
        XCTAssertEqual(height(at: 806), 2)
        XCTAssertEqual(height(at: 440), 1)
        XCTAssertEqual(height(at: 600), 3)
        XCTAssertEqual(height(at: 700), 4)
        XCTAssertEqual(height(at: 806), 5)
    }

    func testWidthScopedCacheBoundsEntriesAtEachWidth() {
        var cache = WidthScopedHeightCache<String>(maximumEntriesPerWidth: 2)
        XCTAssertEqual(cache.value(for: "first", width: 440) { 1 }, 1)
        XCTAssertEqual(cache.value(for: "second", width: 440) { 2 }, 2)
        XCTAssertEqual(cache.value(for: "uncached", width: 440) { 3 }, 3)
        XCTAssertEqual(cache.value(for: "uncached", width: 440) { 4 }, 4)
        XCTAssertEqual(cache.value(for: "first", width: 440) { 5 }, 1)
    }

    func testArticleFooterHeightUsesRenderedTitleWidth() {
        let metrics = OiaCardTextMetrics()
        let title = "Swell Wall Catchall by Anna Dawson — Sculptural Organizer & Hanger"

        XCTAssertGreaterThan(
            metrics.articleFooterHeight(for: title, width: 220),
            metrics.articleFooterHeight(for: title, width: 403)
        )
    }

    func testArticleFooterHeightUsesHardLineBreaks() {
        let metrics = OiaCardTextMetrics()

        XCTAssertGreaterThan(
            metrics.articleFooterHeight(for: "one\ntwo\nthree", width: 403),
            metrics.articleFooterHeight(for: "one two three", width: 403)
        )
    }

    func testArticleFooterHeightStopsAtTheVisibleLineLimit() {
        let metrics = OiaCardTextMetrics()
        let long = String(repeating: "visible words ", count: 100)
        let longer = String(repeating: "visible words ", count: 200)

        XCTAssertEqual(
            metrics.articleFooterHeight(for: long, width: 220),
            metrics.articleFooterHeight(for: longer, width: 220)
        )
    }

    func testArticleFooterHeightMatchesTheRenderedStack() {
        let metrics = OiaCardTextMetrics()

        for title in [
            "Spoke: Last mile delivery software",
            "Swell Wall Catchall by Anna Dawson — Sculptural Organizer & Hanger",
            String(repeating: "A deliberately long title ", count: 20)
        ] {
            XCTAssertEqual(
                metrics.articleFooterHeight(for: title, width: 320),
                renderedArticleFooterHeight(for: title, width: 320),
                accuracy: 0.5
            )
        }
    }

    func testFullArticleHeightMatchesRenderedTextAtDifferentWidths() {
        let metrics = OiaCardTextMetrics()
        let examples: [(String, String?, CGFloat)] = [
            ("The Egg", "A brief story about a life and the people who shared it.", 220),
            (
                "About the Raskin Center for Humane Interfaces",
                "History and mission of the Raskin Center for Humane Interfaces, "
                    + "founded to continue Jef Raskin's work in human-computer interaction.",
                320
            ),
            (
                "Do not remain nameless to yourself",
                String(repeating: "A passage of saved article text. ", count: 20),
                403
            ),
            ("A heading without article text", nil, 220),
            ("A heading with a hard\nline break", "A first line.\nA second line.\nA third line.", 300),
            ("A longunbrokentitlethatmustwrapatthesmallestcardwidth", "A short preview.", 180)
        ]

        for (title, description, width) in examples {
            XCTAssertEqual(
                metrics.fullArticleTextHeight(for: title, description: description, width: width),
                renderedFullArticleTextHeight(for: title, description: description, width: width),
                accuracy: 1,
                "Height mismatch for: \(title)"
            )
        }
    }

    func testFullArticleHeightGrowsOnlyWithVisibleDescriptionLines() {
        let metrics = OiaCardTextMetrics()
        let title = "The Egg"
        let oneLine = "A short description."
        let severalLines = String(repeating: "A saved paragraph with enough text to wrap. ", count: 2)
        let oversized = String(repeating: "A saved paragraph with enough text to wrap. ", count: 30)

        let shortHeight = metrics.fullArticleTextHeight(for: title, description: oneLine, width: 220)
        let severalHeight = metrics.fullArticleTextHeight(for: title, description: severalLines, width: 220)
        let oversizedHeight = metrics.fullArticleTextHeight(for: title, description: oversized, width: 220)
        XCTAssertLessThan(shortHeight, severalHeight)
        XCTAssertLessThanOrEqual(severalHeight, oversizedHeight)
        XCTAssertLessThan(
            metrics.fullArticleTextHeight(for: title, description: severalLines, width: 403),
            severalHeight
        )
    }

    func testQuoteHeightMatchesTheRenderedStack() {
        let metrics = OiaCardTextMetrics()

        for text in [
            "doubt is not a pleasant condition, but certainty is absurd",
            [
                "A solar eclipse occurs when the Moon passes between Earth and the Sun, ",
                "thereby obscuring the view of the Sun from a small part of Earth, ",
                "totally or partially."
            ].joined(),
            String(repeating: "A deliberately long quote ", count: 100)
        ] {
            let measured = metrics.quoteCardHeight(for: text, width: 403, cardSize: .extraLarge)
            let rendered = renderedQuoteHeight(for: text, width: 403, cardSize: .extraLarge)
            XCTAssertEqual(
                measured,
                rendered,
                accuracy: 0.5,
                "Height mismatch for: \(text.prefix(32))"
            )
        }
    }

    func testQuoteHeightMatchesTheRenderedStackAtEveryCardSize() {
        let metrics = OiaCardTextMetrics()
        let texts = [
            "A short quote",
            "Behind your image, below your words, above your thoughts, the silence of another world awaits.",
            Array(repeating: "one full line", count: OiaCardTextMetrics.quoteLineLimit)
                .joined(separator: "\n")
        ]

        for cardSize in CardSize.allCases {
            let width = cardSize.minimumColumnWidth
            for text in texts {
                let measured = metrics.quoteCardHeight(for: text, width: width, cardSize: cardSize)
                let rendered = renderedQuoteHeight(for: text, width: width, cardSize: cardSize)
                XCTAssertEqual(
                    measured,
                    rendered,
                    accuracy: 0.5,
                    "Height mismatch for: \(cardSize.label), \(text.prefix(24))"
                )
            }
        }
    }

    func testQuoteHeightCacheSeparatesCardSizesAtTheSameWidth() {
        let text = Array(repeating: "one full line", count: OiaCardTextMetrics.quoteLineLimit)
            .joined(separator: "\n")
        let width: CGFloat = 403

        for cardSizes in [
            [CardSize.extraSmall, .extraLarge],
            [CardSize.extraLarge, .extraSmall]
        ] {
            let metrics = OiaCardTextMetrics()
            for cardSize in cardSizes {
                let measured = metrics.quoteCardHeight(for: text, width: width, cardSize: cardSize)
                let rendered = renderedQuoteHeight(for: text, width: width, cardSize: cardSize)
                XCTAssertEqual(
                    measured,
                    rendered,
                    accuracy: 0.5
                )
            }
        }
    }

    func testQuoteHeightUsesRenderedWidthAndHardLineBreaks() {
        let metrics = OiaCardTextMetrics()
        let text = "Behind your image, below your words, above your thoughts, the silence of another world awaits."
        let sevenLines = Array(repeating: "one", count: 7).joined(separator: "\n")

        XCTAssertLessThanOrEqual(
            metrics.quoteCardHeight(for: text, width: 403, cardSize: .extraLarge),
            metrics.quoteCardHeight(for: text, width: 220, cardSize: .extraLarge)
        )
        XCTAssertGreaterThan(
            metrics.quoteCardHeight(for: sevenLines, width: 403, cardSize: .extraLarge),
            metrics.quoteCardHeight(
                for: "one two three",
                width: 403,
                cardSize: .extraLarge
            )
        )
    }

    func testQuoteCardsScaleTheirVerticalSpaceWithCardSize() {
        let metrics = OiaCardTextMetrics()
        let text = "Not every situation warrants advice. Sometimes, "
            + "the person just wants to gauge a reaction or to share."

        XCTAssertEqual(OiaCardTextMetrics.quoteVerticalPadding, 20)
        XCTAssertEqual(OiaCardTextMetrics.quoteHorizontalPadding, 20)
        let narrowHeight = metrics.quoteCardHeight(
            for: text,
            width: CardSize.extraSmall.minimumColumnWidth,
            cardSize: .extraSmall
        )
        let wideHeight = metrics.quoteCardHeight(
            for: text,
            width: CardSize.extraLarge.minimumColumnWidth,
            cardSize: .extraLarge
        )
        XCTAssertGreaterThan(narrowHeight, wideHeight)
        XCTAssertLessThan(wideHeight, 300)
    }

    func testQuoteHeightStopsAtTheVisibleLineLimit() {
        let metrics = OiaCardTextMetrics()
        let long = String(repeating: "visible words ", count: 1000)
        let longer = String(repeating: "visible words ", count: 2000)

        XCTAssertEqual(
            metrics.quoteCardHeight(for: long, width: 220, cardSize: .extraLarge),
            metrics.quoteCardHeight(for: longer, width: 220, cardSize: .extraLarge)
        )
    }
}

extension OiaCardTextMetricsTests {
    func testSocialPostHeightTracksTextWidthAndAttachmentRatio() {
        let metrics = OiaCardTextMetrics()
        let text = "A locally saved post keeps its text and media readable even after the source disappears."

        XCTAssertGreaterThan(
            metrics.socialPostCardHeight(
                for: text,
                width: 220,
                attachmentAspectRatio: nil
            ),
            metrics.socialPostCardHeight(
                for: text,
                width: 403,
                attachmentAspectRatio: nil
            )
        )
        XCTAssertGreaterThan(
            metrics.socialPostCardHeight(
                for: text,
                width: 320,
                attachmentAspectRatio: 3.0 / 4.0
            ),
            metrics.socialPostCardHeight(
                for: text,
                width: 320,
                attachmentAspectRatio: 16.0 / 9.0
            )
        )
    }

    func testSocialPostHeightStopsAtVisibleLineLimit() {
        let metrics = OiaCardTextMetrics()
        let long = String(repeating: "visible words ", count: 200)
        let longer = String(repeating: "visible words ", count: 400)

        XCTAssertEqual(
            metrics.socialPostCardHeight(
                for: long,
                width: 220,
                attachmentAspectRatio: 16.0 / 9.0
            ),
            metrics.socialPostCardHeight(
                for: longer,
                width: 220,
                attachmentAspectRatio: 16.0 / 9.0
            )
        )
    }

    func testSocialPostHeightMatchesRenderedStack() {
        let metrics = OiaCardTextMetrics()
        let text = "Local-first social posts preserve the words and the media together."

        let ratios: [CGFloat?] = [nil, 16.0 / 9.0, 3.0 / 4.0]
        for ratio in ratios {
            XCTAssertEqual(
                metrics.socialPostCardHeight(
                    for: text,
                    width: 320,
                    attachmentAspectRatio: ratio
                ),
                renderedSocialPostHeight(
                    for: text,
                    width: 320,
                    attachmentAspectRatio: ratio
                ),
                accuracy: 0.5
            )
        }
    }

    private func renderedArticleFooterHeight(for title: String, width: CGFloat) -> CGFloat {
        let view = VStack(
            alignment: .leading,
            spacing: OiaCardTextMetrics.articleFooterSpacing
        ) {
            Text(title)
                .font(Font(OiaCardTextMetrics.articleTitleFont))
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)
            Color.clear
                .frame(height: OiaCardTextMetrics.articleFooterSourceLineHeight)
        }
        .padding(OiaCardTextMetrics.articleFooterPadding)
        .frame(width: width, alignment: .leading)

        return ceil(NSHostingView(rootView: view).fittingSize.height)
    }

    private func renderedFullArticleTextHeight(
        for title: String,
        description: String?,
        width: CGFloat
    ) -> CGFloat {
        var row = makeReadingRow(wordCount: 2_500)
        row.title = title
        row.cardDescription = description
        row.site = "example.com"
        let view = FullArticleCardText(row: row, libraryURL: nil, isVisible: false)
            .frame(width: width, alignment: .leading)

        return ceil(NSHostingView(rootView: view).fittingSize.height)
    }

    private func renderedQuoteHeight(
        for text: String,
        width: CGFloat,
        cardSize: CardSize
    ) -> CGFloat {
        let view = VStack(alignment: .center, spacing: 0) {
            renderedQuoteMark("“", cardSize: cardSize)
            Text(text)
                .font(Font(OiaCardTextMetrics.quoteFont(for: cardSize)))
                .lineSpacing(OiaCardTextMetrics.quoteLineSpacing)
                .multilineTextAlignment(.center)
                .lineLimit(OiaCardTextMetrics.quoteLineLimit)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .center)
                .padding(.top, OiaCardTextMetrics.quoteMarkSpacing(for: cardSize))
            renderedQuoteMark("”", cardSize: cardSize)
                .padding(.top, OiaCardTextMetrics.quoteMarkSpacing(for: cardSize))
        }
        .padding(.horizontal, OiaCardTextMetrics.quoteHorizontalPadding)
        .padding(.vertical, OiaCardTextMetrics.quoteVerticalPadding)
        .frame(width: width, alignment: .center)

        return ceil(NSHostingView(rootView: view).fittingSize.height)
    }

    private func renderedQuoteMark(_ mark: String, cardSize: CardSize) -> some View {
        Text(mark)
            .font(Font(OiaCardTextMetrics.quoteMarkFont(for: cardSize)))
            .fixedSize()
            .offset(y: OiaCardTextMetrics.quoteMarkVerticalOffset(for: cardSize))
            .frame(
                maxWidth: .infinity,
                minHeight: OiaCardTextMetrics.quoteMarkHeight(for: cardSize),
                maxHeight: OiaCardTextMetrics.quoteMarkHeight(for: cardSize),
                alignment: .center
            )
    }

    private func renderedSocialPostHeight(
        for text: String,
        width: CGFloat,
        attachmentAspectRatio: CGFloat?
    ) -> CGFloat {
        let contentWidth = width - OiaCardTextMetrics.socialPostPadding * 2
        let view = VStack(
            alignment: .leading,
            spacing: OiaCardTextMetrics.socialPostSpacing
        ) {
            Color.clear
                .frame(height: OiaCardTextMetrics.socialPostHeaderHeight)
            Text(text)
                .font(Font(OiaCardTextMetrics.socialPostFont))
                .lineSpacing(OiaCardTextMetrics.socialPostLineSpacing)
                .lineLimit(OiaCardTextMetrics.socialPostLineLimit)
                .fixedSize(horizontal: false, vertical: true)
            if let attachmentAspectRatio {
                Color.clear
                    .frame(height: contentWidth / attachmentAspectRatio)
            }
        }
        .padding(OiaCardTextMetrics.socialPostPadding)
        .frame(width: width, alignment: .leading)

        return ceil(NSHostingView(rootView: view).fittingSize.height)
    }
}
