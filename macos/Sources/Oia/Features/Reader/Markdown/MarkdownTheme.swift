// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Derives the native reader's type and spacing from the user's preferences.
/// The default rhythm follows iA Writer's Classic preview: a serif text column,
/// generous space between paragraphs, and a quiet heading hierarchy. The
/// reader's font, size, width, and leading remain adjustable per device.
///
/// All values are expressed *relative to the body size* so the whole document
/// rescales when the reader picks Small … Giant. See DESIGN.md →
/// "Apple platform style guide" for the rationale behind each token.
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

    /// The open paragraph rhythm in the Classic preview. Lists override this
    /// with compact item spacing; headings add extra space above themselves.
    var blockSpacing: CGFloat {
        bodySize * 1.5
    }

    /// The reader's measure, from the reader's chosen width. The Medium default
    /// (680) is the optimal line length for sustained reading (~60–75 characters).
    var contentMaxWidth: CGFloat {
        width.points
    }

    var bodyFont: Font {
        font.swiftUIFont(size: bodySize)
    }

    var codeFont: Font {
        .system(size: bodySize * 0.9, design: .monospaced)
    }

    // ── Headings ────────────────────────────────────────────────────────────
    // A restrained hierarchy: titles and section headings change size only
    // slightly. Lower levels distinguish themselves by weight and italic style.

    func headingSize(_ level: Int) -> CGFloat {
        switch level {
        case 1: bodySize * 1.25
        case 2: bodySize * 1.12
        default: bodySize
        }
    }

    func headingWeight(_ level: Int) -> Font.Weight {
        switch level {
        case 1, 2, 3, 6: .bold
        default: .semibold
        }
    }

    func headingFont(_ level: Int) -> Font {
        font.swiftUIFont(size: headingSize(level), weight: headingWeight(level),
                         italic: headingIsItalic(level))
    }

    /// Classic keeps the natural spacing of the chosen text face.
    func headingTracking(_ level: Int) -> CGFloat {
        0
    }

    func headingIsItalic(_ level: Int) -> Bool {
        level == 4 || level == 5
    }

    /// H6 sits on the same line as the following paragraph when one exists.
    func headingIsRunIn(_ level: Int) -> Bool {
        level == 6
    }

    /// Space *above* a heading. Larger for higher levels so major sections get a
    /// clear visual break; this is added on top of the inter-block `blockSpacing`.
    func headingSpaceAbove(_ level: Int) -> CGFloat {
        switch level {
        case 1: bodySize * 1.0
        case 2: bodySize * 0.9
        case 3: bodySize * 0.7
        default: bodySize * 0.45
        }
    }

    // ── Lists ───────────────────────────────────────────────────────────────

    /// Gap between sibling list items (tighter than `blockSpacing`).
    var listItemSpacing: CGFloat {
        bodySize * 0.18
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

    /// Classic uses the same quiet bullet at every nesting depth; indentation
    /// carries the hierarchy.
    func bullet(depth: Int) -> String {
        "•"
    }

    // ── Block quote ─────────────────────────────────────────────────────────

    var quoteIndent: CGFloat {
        bodySize * 2.1
    }

    var quoteInnerSpacing: CGFloat {
        blockSpacing * 0.6
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

    /// The header title is the reading's sole h1 — the top of the heading scale —
    /// so it borrows the level-1 heading tokens (and, like the body, follows the
    /// chosen reader font).
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
