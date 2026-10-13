// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Native adaptation of NetNewsWire's Biblioteca theme: platform serif prose,
/// sans-serif section headings, compact paragraph rhythm, and quiet quotes.
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

    /// Browser-default paragraph margins collapse to one body em.
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
    // Biblioteca uses a bold serif H1 and bold sans-serif section headings.

    func headingSize(_ level: Int) -> CGFloat {
        switch level {
        case 1: bodySize * 2
        case 2: bodySize * 1.5
        case 3: bodySize * 1.17
        case 4: bodySize
        case 5: bodySize * 0.83
        default: bodySize * 0.67
        }
    }

    func headingWeight(_: Int) -> Font.Weight {
        .bold
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

    /// Space *above* a heading. Larger for higher levels so major sections get a
    /// clear visual break; this is added on top of the inter-block `blockSpacing`.
    func headingSpaceAbove(_ level: Int) -> CGFloat {
        max(headingSize(level) * 1.5 - blockSpacing, 0)
    }

    func headingReaderFont(_ level: Int) -> ReaderFont {
        level == 1 ? .serif : .system
    }

    func headingLineHeight(_ level: Int) -> CGFloat {
        headingSize(level) * 1.2
    }

    func headingSpaceBelow(_ level: Int) -> CGFloat {
        headingSize(level) * 0.25
    }

    var quoteSize: CGFloat {
        bodySize
    }

    var quoteLineHeight: CGFloat {
        bodySize * lineHeight.multiple
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
        bodySize * 0.5
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
        18
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

    /// Give the article title more presence than headings within the body while
    /// keeping it tied to the chosen reader font and body size.
    var titleFont: Font {
        ReaderFont.serif.swiftUIFont(size: headingSize(1), weight: .bold)
    }

    var titleTracking: CGFloat {
        headingTracking(1)
    }

    /// The reading-time and tags line follows the reader font at a quieter size.
    var metadataFont: Font {
        font.swiftUIFont(size: bodySize * 0.85)
    }
}
