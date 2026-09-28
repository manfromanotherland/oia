// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// The reader's measure and leading, which `MarkdownTheme` derives from the
/// user's Width and Line Height preferences. The tests pin the intended reader
/// defaults and the available preference stops.
final class MarkdownThemeTests: XCTestCase {
    private func theme(
        width: ReaderWidth = .medium,
        lineHeight: ReaderLineHeight = .normal,
        fontSize: ReaderFontSize = .medium
    ) -> MarkdownTheme {
        MarkdownTheme(font: .defaultChoice, fontSize: fontSize, width: width, lineHeight: lineHeight)
    }

    // ── Defaults ────────────────────────────────────────────────────────────

    /// Medium / Normal use a 680 pt measure and 0.3em of added leading,
    /// giving the body a line height of approximately 1.5.
    func testDefaultMeasureAndLeading() {
        let theme = theme()
        XCTAssertEqual(theme.contentMaxWidth, 680)
        XCTAssertEqual(theme.lineSpacing, theme.bodySize * 0.3, accuracy: 0.001)
    }

    func testDefaultFontIsSerif() {
        XCTAssertEqual(ReaderFont.defaultChoice, .serif)
    }

    func testClassicArticleHierarchyKeepsHeadingsCloseToBodySize() {
        let theme = theme()
        XCTAssertEqual(theme.blockSpacing, theme.bodySize * 1.5, accuracy: 0.001)
        XCTAssertEqual(theme.headingSize(1), theme.bodySize * 1.25, accuracy: 0.001)
        XCTAssertEqual(theme.headingSize(2), theme.bodySize * 1.12, accuracy: 0.001)
        XCTAssertEqual(theme.headingSize(3), theme.bodySize, accuracy: 0.001)
        XCTAssertTrue(theme.headingIsItalic(4))
        XCTAssertTrue(theme.headingIsItalic(5))
        XCTAssertTrue(theme.headingIsRunIn(6))
        XCTAssertEqual(theme.quoteIndent, theme.bodySize * 2.1, accuracy: 0.001)
        XCTAssertEqual(theme.bullet(depth: 0), theme.bullet(depth: 2))
        XCTAssertLessThan(theme.listItemSpacing, theme.blockSpacing)
    }

    // ── Width ───────────────────────────────────────────────────────────────

    func testWidthDrivesTheContentMeasure() {
        XCTAssertEqual(theme(width: .xsmall).contentMaxWidth, 520)
        XCTAssertEqual(theme(width: .small).contentMaxWidth, 600)
        XCTAssertEqual(theme(width: .medium).contentMaxWidth, 680)
        XCTAssertEqual(theme(width: .large).contentMaxWidth, 800)
        XCTAssertEqual(theme(width: .xlarge).contentMaxWidth, 960)
    }

    func testWidthOptionsAreOrderedNarrowToWide() {
        let measures = ReaderWidth.allCases.map(\.points)
        XCTAssertEqual(measures, measures.sorted(), "the picker reads XS → XL")
        XCTAssertEqual(Set(measures).count, measures.count, "no two options share a measure")
    }

    /// The measure is a fixed point value, so bumping the text size must not
    /// widen the column — otherwise "bigger text" silently becomes "wider page".
    func testMeasureIsIndependentOfFontSize() {
        for size in ReaderFontSize.allCases {
            XCTAssertEqual(theme(width: .large, fontSize: size).contentMaxWidth, 800,
                           "measure held at \(size.label)")
        }
    }

    // ── Line height ─────────────────────────────────────────────────────────

    /// Both SwiftUI and AppKit add `lineSpacing` on top of the font's natural
    /// ~1.2× line height, so the theme emits the *difference*, not the full
    /// multiple.
    func testLineSpacingIsTheLeadingAboveTheNaturalLineHeight() {
        for lineHeight in ReaderLineHeight.allCases {
            let theme = theme(lineHeight: lineHeight)
            XCTAssertEqual(theme.lineSpacing,
                           theme.bodySize * (lineHeight.multiple - 1.2),
                           accuracy: 0.001,
                           "\(lineHeight.label) leading")
        }
    }

    func testLineHeightOptionsAreOrderedTightToLoose() {
        let multiples = ReaderLineHeight.allCases.map(\.multiple)
        XCTAssertEqual(multiples, multiples.sorted(), "the slider reads Tight → Loose")
        XCTAssertEqual(Set(multiples).count, multiples.count, "no two options share a line height")
    }

    /// Both scales have five stops, so the two popover sliders share a shape.
    func testBothScalesHaveFiveStops() {
        XCTAssertEqual(ReaderWidth.allCases.count, 5)
        XCTAssertEqual(ReaderLineHeight.allCases.count, 5)
    }

    /// The defaults sit at the middle stop of each scale, so a slider dropped at
    /// centre lands on the shipped default.
    func testDefaultsAreTheMiddleStop() {
        XCTAssertEqual(ReaderWidth.allCases[2], .medium)
        XCTAssertEqual(ReaderLineHeight.allCases[2], .normal)
    }

    /// The first two stops offer subtle adjustments around the normal reading
    /// rhythm; the last two provide more room for readers who prefer it.
    func testLineHeightStopMultiples() {
        let multiples = ReaderLineHeight.allCases.map(\.multiple)
        let expected: [CGFloat] = [1.2, 1.35, 1.5, 1.75, 2.0]
        zip(multiples, expected).forEach { actual, intended in
            XCTAssertEqual(actual, intended, accuracy: 0.001)
        }
    }

    /// Guards the `max(…, 0)` floor: a line height at or under the natural 1.2×
    /// must clamp to zero added leading rather than going negative and
    /// overlapping lines.
    func testAddedLeadingNeverGoesNegative() {
        for lineHeight in ReaderLineHeight.allCases {
            XCTAssertGreaterThanOrEqual(lineHeight.extraLeadingMultiple, 0, lineHeight.label)
        }
    }

    /// Leading scales with the body size, so the rhythm holds at any text size.
    func testLeadingScalesWithBodySize() {
        let small = theme(lineHeight: .loose, fontSize: .small)
        let giant = theme(lineHeight: .loose, fontSize: .giant)
        XCTAssertGreaterThan(giant.lineSpacing, small.lineSpacing)
        XCTAssertEqual(giant.lineSpacing / giant.bodySize,
                       small.lineSpacing / small.bodySize,
                       accuracy: 0.001)
    }

    // ── Persistence vocabulary ──────────────────────────────────────────────

    /// `@AppStorage` persists these by raw value, so a renamed or reordered raw
    /// value would silently reset every user's saved choice.
    func testRawValuesAreStableForPersistence() {
        XCTAssertEqual(ReaderWidth.allCases.map(\.rawValue), [520, 600, 680, 800, 960])
        XCTAssertEqual(ReaderLineHeight.allCases.map(\.rawValue),
                       ["tight", "snug", "normal", "relaxed", "loose"])
    }

    func testEveryOptionHasADistinctLabel() {
        XCTAssertEqual(Set(ReaderWidth.allCases.map(\.label)).count, ReaderWidth.allCases.count)
        XCTAssertEqual(Set(ReaderLineHeight.allCases.map(\.label)).count, ReaderLineHeight.allCases.count)
    }
}
