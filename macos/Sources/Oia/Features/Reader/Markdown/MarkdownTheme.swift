// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Native rendering of the selected typography playground theme: Helvetica Neue
/// prose, Fraunces headings and block quotes, and the exported reading rhythm.
/// The user's font, size, width, and leading preferences remain per-device.
struct MarkdownTheme {
    let font: ReaderFont
    let fontSize: ReaderFontSize
    let width: ReaderWidth
    let lineHeight: ReaderLineHeight

    init(font: ReaderFont, fontSize: ReaderFontSize,
         width: ReaderWidth = .medium, lineHeight: ReaderLineHeight = .normal)
    {
        self.font = font
        self.fontSize = fontSize
        self.width = width
        self.lineHeight = lineHeight
    }

    var design: Font.Design {
        font.design
    }

    var bodySize: CGFloat {
        fontSize.points
    }

    // ── Reading rhythm ──────────────────────────────────────────────────────

    /// SwiftUI `lineSpacing` is the gap *between* lines, added on top of the
    /// natural (~1.2×) line height, so the reader's chosen line height becomes
    /// the *extra* leading over that baseline (see `extraLeadingMultiple`).
    var lineSpacing: CGFloat {
        bodySize * lineHeight.extraLeadingMultiple
    }

    /// The selected 18 pt paragraph gap scales with the body preference.
    var blockSpacing: CGFloat {
        bodySize
    }

    /// The reader's measure, from the reader's chosen width. The Medium default
    /// (680) is the optimal line length for sustained reading (~60–75 characters).
    var contentMaxWidth: CGFloat {
        width.points
    }

    var bodyFont: Font {
        font.swiftUIFont(size: bodySize, weight: .regular)
    }

    var codeFont: Font {
        .system(size: bodySize * 0.9, design: .monospaced)
    }

    // ── Headings ────────────────────────────────────────────────────────────
    // The exported H1 is 48 pt; all heading levels share Fraunces.

    func headingSize(_ level: Int) -> CGFloat {
        let ratios: [CGFloat] = [1, 0.75, 0.6, 0.5, 0.44, 0.4]
        return bodySize * (48 / 18) * ratios[min(max(level, 1), 6) - 1]
    }

    func headingWeight(_: Int) -> Font.Weight {
        .regular
    }

    func headingFont(_ level: Int) -> Font {
        headingReaderFont(level).swiftUIFont(size: headingSize(level), weight: headingWeight(level),
                                             italic: headingIsItalic(level))
    }

    /// The reference uses natural letter spacing.
    func headingTracking(_: Int) -> CGFloat {
        0
    }

    func headingIsItalic(_: Int) -> Bool {
        false
    }

    /// Every heading starts its own block.
    func headingIsRunIn(_: Int) -> Bool {
        false
    }

    /// The exported 64 pt section gap includes the normal inter-block spacing.
    func headingSpaceAbove(_: Int) -> CGFloat {
        max(bodySize * (64 / 18) - blockSpacing, 0)
    }

    func headingReaderFont(_: Int) -> ReaderFont {
        .fraunces
    }

    func headingLineHeight(_ level: Int) -> CGFloat {
        headingSize(level) * 1.2
    }

    func headingSpaceBelow(_: Int) -> CGFloat {
        bodySize * (10 / 18)
    }

    var quoteSize: CGFloat {
        bodySize * (28 / 18)
    }

    var quoteLineHeight: CGFloat {
        quoteSize * 1.2
    }

    var quoteReaderFont: ReaderFont {
        .fraunces
    }

    var titleBodyGap: CGFloat {
        bodySize * (24 / 18)
    }

    var quoteRuleWidth: CGFloat {
        3
    }

    var quoteSpaceAbove: CGFloat {
        blockSpacing
    }

    // ── Lists ───────────────────────────────────────────────────────────────

    /// Gap between sibling list items (tighter than `blockSpacing`).
    var listItemSpacing: CGFloat {
        bodySize * 0.35
    }

    /// Gap between the marker (bullet/number) and the item text.
    var listMarkerGap: CGFloat {
        bodySize * 0.4
    }

    /// Reserved width for the marker column, giving a clean hanging indent.
    /// Wide enough for a two-digit ordinal or a checkbox glyph without clipping.
    func listMarkerWidth(ordered: Bool) -> CGFloat {
        bodySize * (ordered ? 1.5 : 1.1)
    }

    /// Use the same quiet bullet at every nesting depth; indentation
    /// carries the hierarchy.
    func bullet(depth _: Int) -> String {
        "•"
    }

    // ── Block quote ─────────────────────────────────────────────────────────

    var quoteIndent: CGFloat {
        24
    }

    var quoteInnerSpacing: CGFloat {
        blockSpacing
    }

    // ── Code ────────────────────────────────────────────────────────────────

    var codePadding: CGFloat {
        14
    }

    // ── Rules, images, captions ───────────────────────────────────────────────

    var ruleSpacing: CGFloat {
        bodySize * 0.35
    }

    var imageCornerRadius: CGFloat {
        6
    }

    var captionGap: CGFloat {
        bodySize * 0.4
    }

    var captionSize: CGFloat {
        bodySize * 0.85
    }

    var captionFont: Font {
        font.swiftUIFont(size: captionSize)
    }

    // ── Article chrome (header title and metadata) ───────────────────────────
    // The reader's non-body pieces are sized from the body so they rescale with
    // it (Small … Giant) instead of holding a fixed size while the copy grows.

    /// The article title uses the exported H1 size and Fraunces face.
    var titleFont: Font {
        headingFont(1)
    }

    var titleTracking: CGFloat {
        headingTracking(1)
    }

    /// The reading-time and tags line follows the reader font at a quieter size.
    var metadataFont: Font {
        font.swiftUIFont(size: bodySize * 0.85)
    }
}
