// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import CoreText
import XCTest

@MainActor
final class NewsreaderFontTests: XCTestCase {
    private let weightAxis = NSNumber(value: UInt32(0x7767_6874))
    private let opticalSizeAxis = NSNumber(value: UInt32(0x6F70_737A))
    private let resourceName = "Newsreader-VariableFont_opsz-wght.ttf"

    func testQuoteTextUsesItalicGeorgiaAndMarksUseBundledNewsreader() throws {
        assertItalicGeorgia(OiaCardTextMetrics.quoteFont(for: .extraLarge), pointSize: 24)
        try assertBundledNewsreader(
            OiaCardTextMetrics.quoteMarkFont(for: .extraLarge),
            pointSize: 59,
            opticalSize: 6
        )
    }

    func testQuoteTypographyScalesWithCardSize() throws {
        let pointSizes = CardSize.allCases.map(OiaCardTextMetrics.quotePointSize(for:))

        XCTAssertEqual(pointSizes, [16, 18, 20, 22, 24])
        for (cardSize, pointSize) in zip(CardSize.allCases, pointSizes) {
            let scale = pointSize / 24
            assertItalicGeorgia(OiaCardTextMetrics.quoteFont(for: cardSize), pointSize: pointSize)
            try assertBundledNewsreader(
                OiaCardTextMetrics.quoteMarkFont(for: cardSize),
                pointSize: 59 * scale,
                opticalSize: 6
            )
            XCTAssertEqual(OiaCardTextMetrics.quoteMarkHeight(for: cardSize), 15 * scale)
            XCTAssertEqual(OiaCardTextMetrics.quoteMarkVerticalOffset(for: cardSize), 21 * scale)
            XCTAssertEqual(OiaCardTextMetrics.quoteMarkSpacing(for: cardSize), 23 * scale)
        }
    }

    func testQuoteDetailUsesCardFontFamiliesAtLargerSizes() throws {
        let pointSize = OiaCardTextMetrics.quoteDetailPointSize
        XCTAssertGreaterThan(pointSize, OiaCardTextMetrics.quotePointSize(for: .extraLarge))
        assertItalicGeorgia(OiaCardTextMetrics.quoteDetailFont, pointSize: pointSize)
        try assertBundledNewsreader(
            OiaCardTextMetrics.quoteDetailMarkFont,
            pointSize: 59 * pointSize / 24,
            opticalSize: 6
        )
    }

    func testNewsreaderContainsDistinctCurlyQuoteGlyphs() {
        let font = OiaCardTextMetrics.quoteMarkFont(for: .extraLarge) as CTFont
        var characters: [UniChar] = [0x201C, 0x201D]
        var glyphs = [CGGlyph](repeating: 0, count: characters.count)

        XCTAssertTrue(CTFontGetGlyphsForCharacters(font, &characters, &glyphs, characters.count))
        XCTAssertNotEqual(glyphs[0], 0)
        XCTAssertNotEqual(glyphs[1], 0)
        XCTAssertNotEqual(glyphs[0], glyphs[1])
    }

    private func assertItalicGeorgia(_ font: NSFont, pointSize: CGFloat) {
        XCTAssertEqual(font.fontName, "Georgia-Italic")
        XCTAssertEqual(font.pointSize, pointSize)
    }

    private func assertBundledNewsreader(
        _ font: NSFont,
        pointSize: CGFloat,
        opticalSize: Double
    ) throws {
        let bundledURL = try XCTUnwrap(
            Bundle(for: NewsreaderFontTests.self).url(
                forResource: "Newsreader-VariableFont_opsz-wght",
                withExtension: "ttf"
            )
        )

        XCTAssertEqual(font.familyName, "Newsreader")
        XCTAssertEqual(font.pointSize, pointSize)
        XCTAssertEqual(bundledURL.lastPathComponent, resourceName)
        XCTAssertEqual(fontURL(for: font)?.lastPathComponent, resourceName)
        XCTAssertEqual(resolvedVariationValue(for: weightAxis, in: font), 300)
        XCTAssertEqual(resolvedVariationValue(for: opticalSizeAxis, in: font), opticalSize)
    }

    private func resolvedVariationValue(for axis: NSNumber, in font: NSFont) -> Double? {
        let variations = CTFontCopyVariation(font as CTFont) as? [NSNumber: NSNumber]
        if let explicitValue = variations?[axis]?.doubleValue {
            return explicitValue
        }

        let axes = CTFontCopyVariationAxes(font as CTFont) as? [[CFString: Any]]
        let definition = axes?.first { definition in
            (definition[kCTFontVariationAxisIdentifierKey] as? NSNumber) == axis
        }
        return (definition?[kCTFontVariationAxisDefaultValueKey] as? NSNumber)?.doubleValue
    }

    private func fontURL(for font: NSFont) -> URL? {
        CTFontCopyAttribute(font as CTFont, kCTFontURLAttribute) as? URL
    }
}
