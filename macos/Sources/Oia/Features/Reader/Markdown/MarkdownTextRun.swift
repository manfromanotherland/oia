// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Markdown
import SwiftUI

/// Coalesces a run of contiguous *text* blocks — headings, text-only
/// paragraphs, lists, and block quotes — into a single `NSAttributedString` so
/// an `NSTextView` can offer continuous selection across them. The theme's
/// reading rhythm (line spacing, inter-block gaps, heading space-above) and its
/// structural layout (list marker columns via tab stops, quote indentation) are
/// re-expressed here as `NSParagraphStyle` attributes, since a single text view
/// has no per-block SwiftUI modifiers.
enum MarkdownTextRun {
    /// One laid-out paragraph: its text (without a trailing newline) and the
    /// paragraph style to apply over it. `spacingBefore` is the gap from the
    /// previous paragraph; the caller fixes the run's very first paragraph.
    private struct Para {
        var content: NSAttributedString
        var style: NSMutableParagraphStyle
        var spacingBefore: CGFloat
    }

    /// Context threaded down the block tree: the current left indent (points),
    /// list nesting depth (drives bullet glyphs), and inherited quote italics.
    private struct Ctx {
        var indent: CGFloat = 0
        var listDepth: Int = 0
        var color: NSColor = .labelColor
        var italic = false
    }

    static func attributed(_ blocks: [Markup], theme: MarkdownTheme) -> NSAttributedString {
        let paras = emitSequence(blocks, theme: theme, ctx: Ctx(),
                                 innerGap: theme.blockSpacing, topLevel: true)

        let out = NSMutableAttributedString()
        for (index, para) in paras.enumerated() {
            let start = out.length
            out.append(para.content)
            // Terminate every paragraph but the last; the newline belongs to the
            // current paragraph's range so its style covers the terminator.
            if index < paras.count - 1 {
                out.append(NSAttributedString(string: "\n"))
            }
            para.style.paragraphSpacingBefore = para.spacingBefore
            out.addAttribute(.paragraphStyle, value: para.style,
                             range: NSRange(location: start, length: out.length - start))
        }
        return out
    }

    // ── Block emitters ───────────────────────────────────────────────────────

    private static func emit(_ block: Markup, theme: MarkdownTheme, ctx: Ctx) -> [Para] {
        switch block {
        case let heading as Heading:
            return [headingParagraph(heading, theme: theme, ctx: ctx)]
        case let paragraph as Paragraph:
            return [textParagraph(paragraph, theme: theme, ctx: ctx)]
        case let list as UnorderedList:
            return emitList(Array(list.listItems), ordered: false, start: 1, theme: theme, ctx: ctx)
        case let list as OrderedList:
            return emitList(Array(list.listItems), ordered: true, start: Int(list.startIndex),
                            theme: theme, ctx: ctx)
        case let quote as BlockQuote:
            var qctx = ctx
            qctx.indent += theme.quoteIndent
            qctx.italic = true
            return emitSequence(quote.blockChildren.map { $0 as Markup }, theme: theme,
                                ctx: qctx, innerGap: theme.quoteInnerSpacing)
        default:
            // Foldability is checked before we get here, so this is only reached
            // for unexpected containers; recurse so nothing is dropped.
            return emitSequence(Array(block.children), theme: theme, ctx: ctx,
                                innerGap: theme.blockSpacing)
        }
    }

    /// Emit sibling blocks, pairing an H6 with a following paragraph at any
    /// nesting depth. The first paragraph keeps its parent's leading gap; only
    /// the top level adds heading space above its own blocks.
    private static func emitSequence(_ blocks: [Markup], theme: MarkdownTheme,
                                     ctx: Ctx, innerGap: CGFloat,
                                     topLevel: Bool = false) -> [Para]
    {
        var result: [Para] = []
        var index = 0
        while index < blocks.count {
            let block = blocks[index]
            let emitted: [Para]
            if let heading = block as? Heading,
               theme.headingIsRunIn(heading.level),
               index + 1 < blocks.count,
               let paragraph = blocks[index + 1] as? Paragraph
            {
                emitted = [runInParagraph(heading, paragraph: paragraph,
                                         theme: theme, ctx: ctx)]
                index += 2
            } else {
                emitted = emit(block, theme: theme, ctx: ctx)
                index += 1
            }
            var paras = emitted
            guard !paras.isEmpty else { continue }
            // The enclosing LazyVStack provides the first top-level block gap.
            // Subsequent siblings use their container's rhythm; top-level
            // headings retain their extra space above.
            let lead = result.isEmpty ? 0 : innerGap
            let headingExtra = topLevel ? (block as? Heading).map { theme.headingSpaceAbove($0.level) } ?? 0 : 0
            paras[0].spacingBefore = lead + headingExtra
            result.append(contentsOf: paras)
        }
        return result
    }

    private static func emitList(_ items: [ListItem], ordered: Bool, start: Int,
                                 theme: MarkdownTheme, ctx: Ctx) -> [Para]
    {
        let markerWidth = theme.listMarkerWidth(ordered: ordered)
        let bodyIndent = ctx.indent + markerWidth + theme.listMarkerGap

        var childCtx = ctx
        childCtx.indent = bodyIndent
        childCtx.listDepth = ctx.listDepth + 1

        var result: [Para] = []
        for (index, item) in items.enumerated() {
            var itemParas = emitSequence(item.blockChildren.map { $0 as Markup }, theme: theme,
                                         ctx: childCtx, innerGap: theme.blockSpacing * 0.5)
            if itemParas.isEmpty {
                itemParas = [textPlaceholder(ctx: childCtx, theme: theme)]
            }

            if item.checkbox == .checked, Array(item.blockChildren).first is Paragraph {
                // A completed task strikes its label, not any nested list or
                // quoted blocks that happen to follow the label.
                let content = NSMutableAttributedString(attributedString: itemParas[0].content)
                content.addAttributes([
                    .strikethroughStyle: NSUnderlineStyle.single.rawValue,
                    .foregroundColor: NSColor.secondaryLabelColor
                ], range: NSRange(location: 0, length: content.length))
                itemParas[0].content = content
            }

            // Prepend the marker to the item's first paragraph, then give that
            // paragraph a hanging-indent style.
            let marker = markerString(ordered: ordered, number: start + index,
                                      item: item, depth: ctx.listDepth, theme: theme)
            let line = NSMutableAttributedString(string: "\t")
            line.append(marker)
            line.append(NSAttributedString(string: "\t"))
            line.append(itemParas[0].content)
            itemParas[0].content = line

            applyHangingIndent(itemParas[0].style, ctx: ctx, markerWidth: markerWidth, bodyIndent: bodyIndent)

            itemParas[0].spacingBefore = index == 0 ? 0 : theme.listItemSpacing
            result.append(contentsOf: itemParas)
        }
        return result
    }

    /// Give a list item's first paragraph a hanging indent: a right tab
    /// right-aligns the marker in its column, a left tab starts the text at
    /// `bodyIndent`, and `headIndent` keeps wrapped lines under the text.
    private static func applyHangingIndent(_ style: NSMutableParagraphStyle, ctx: Ctx,
                                           markerWidth: CGFloat, bodyIndent: CGFloat)
    {
        style.firstLineHeadIndent = ctx.indent
        style.headIndent = bodyIndent
        style.tabStops = [
            NSTextTab(textAlignment: .right, location: ctx.indent + markerWidth, options: [:]),
            NSTextTab(textAlignment: .left, location: bodyIndent, options: [:])
        ]
        style.defaultTabInterval = bodyIndent
    }

    // ── Leaf paragraphs ────────────────────────────────────────────────────────

    private static func textParagraph(_ paragraph: Markup, theme: MarkdownTheme, ctx: Ctx) -> Para {
        let content = NSMutableAttributedString(attributedString:
            AppKitInline.attributed(paragraph, size: theme.bodySize, weight: .regular,
                                    design: theme.design, color: ctx.color, italic: ctx.italic))
        let style = baseStyle(indent: ctx.indent)
        style.lineSpacing = theme.lineSpacing
        return Para(content: content, style: style, spacingBefore: 0)
    }

    private static func headingParagraph(_ heading: Heading, theme: MarkdownTheme, ctx: Ctx) -> Para {
        let level = heading.level
        let content = NSMutableAttributedString(attributedString:
            AppKitInline.attributed(heading, size: theme.headingSize(level),
                                    weight: theme.headingWeight(level), design: theme.design,
                                    color: ctx.color, italic: ctx.italic || theme.headingIsItalic(level)))
        let tracking = theme.headingTracking(level)
        if tracking != 0 {
            content.addAttribute(.kern, value: tracking,
                                 range: NSRange(location: 0, length: content.length))
        }
        let style = baseStyle(indent: ctx.indent)
        return Para(content: content, style: style, spacingBefore: 0)
    }

    /// Classic renders its smallest heading as a bold label on the next
    /// paragraph's first line. Combining both nodes preserves continuous text
    /// selection and keeps the paragraph's normal wrapping and leading.
    private static func runInParagraph(_ heading: Heading, paragraph: Paragraph,
                                       theme: MarkdownTheme, ctx: Ctx) -> Para
    {
        let content = NSMutableAttributedString(attributedString:
            headingParagraph(heading, theme: theme, ctx: ctx).content)
        content.append(NSAttributedString(string: " ", attributes: [
            .font: AppKitInline.makeFont(size: theme.bodySize, weight: .regular,
                                          design: theme.design, bold: false,
                                          italic: ctx.italic),
            .foregroundColor: ctx.color
        ]))
        content.append(AppKitInline.attributed(paragraph, size: theme.bodySize,
                                               weight: .regular, design: theme.design,
                                               color: ctx.color, italic: ctx.italic))
        let style = baseStyle(indent: ctx.indent)
        style.lineSpacing = theme.lineSpacing
        return Para(content: content, style: style, spacingBefore: 0)
    }

    /// An empty list item still needs a line so its marker renders.
    private static func textPlaceholder(ctx: Ctx, theme: MarkdownTheme) -> Para {
        let style = baseStyle(indent: ctx.indent)
        style.lineSpacing = theme.lineSpacing
        return Para(content: NSAttributedString(string: ""), style: style, spacingBefore: 0)
    }

    // ── Helpers ────────────────────────────────────────────────────────────────

    private static func baseStyle(indent: CGFloat) -> NSMutableParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byWordWrapping
        style.firstLineHeadIndent = indent
        style.headIndent = indent
        return style
    }

    private static func markerString(ordered: Bool, number: Int, item: ListItem,
                                     depth: Int, theme: MarkdownTheme) -> NSAttributedString
    {
        if let checkbox = item.checkbox {
            let checked = checkbox == .checked
            let font = AppKitInline.makeFont(size: theme.bodySize, weight: .regular,
                                             design: theme.design, bold: false, italic: false)
            return NSAttributedString(string: checked ? "☑" : "☐", attributes: [
                .font: font,
                .foregroundColor: NSColor.secondaryLabelColor
            ])
        }
        let glyph = ordered ? "\(number)." : theme.bullet(depth: depth)
        let font = ordered
            ? NSFont.monospacedDigitSystemFont(ofSize: theme.bodySize, weight: .regular)
            : AppKitInline.makeFont(size: theme.bodySize, weight: .regular,
                                    design: theme.design, bold: false, italic: false)
        return NSAttributedString(string: glyph, attributes: [
            .font: font,
            .foregroundColor: NSColor.secondaryLabelColor
        ])
    }
}
