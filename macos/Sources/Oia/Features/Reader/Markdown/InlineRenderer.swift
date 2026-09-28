// SPDX-License-Identifier: GPL-3.0-or-later

import Markdown
import SwiftUI

/// `Text` collides between SwiftUI and Markdown; alias the bare name to SwiftUI
/// so `Text.LineStyle` resolves. Markdown's node is used as `Markdown.Text`.
private typealias Text = SwiftUI.Text

/// Flattens swift-markdown inline nodes into a styled `AttributedString` for use
/// in a SwiftUI `Text`. Font traits (bold/italic/code) and link/strike state are
/// carried down the recursion and applied only at text leaves, so nested
/// emphasis combines correctly.
///
/// Every run gets an explicit `.font` attribute, which **overrides** any
/// `.font()` view modifier on the enclosing `Text`. The base font therefore has
/// to be injected here via `FontContext` — passing a heading's size/weight so
/// headings actually render as headings rather than inheriting body text.
enum InlineRenderer {
    /// The base font a run inherits before inline emphasis (bold/italic/code) is
    /// layered on top. Callers pass `.body` for running text or `.heading(...)`
    /// / a custom context for headings and table headers.
    struct FontContext {
        var size: CGFloat
        var weight: Font.Weight = .regular
        var font: ReaderFont = .defaultChoice
        var italic = false

        static func body(_ theme: MarkdownTheme, italic: Bool = false) -> FontContext {
            FontContext(size: theme.bodySize, weight: .regular, font: theme.font,
                        italic: italic)
        }

        static func heading(_ level: Int, _ theme: MarkdownTheme) -> FontContext {
            FontContext(size: theme.headingSize(level),
                        weight: theme.headingWeight(level),
                        font: theme.font,
                        italic: theme.headingIsItalic(level))
        }

        static func emphasized(_ theme: MarkdownTheme, weight: Font.Weight) -> FontContext {
            FontContext(size: theme.bodySize, weight: weight, font: theme.font)
        }
    }

    struct Style {
        var bold = false
        var italic = false
        var strike = false
        var link: URL?

        func with(bold: Bool? = nil, italic: Bool? = nil,
                  strike: Bool? = nil, link: URL? = nil) -> Style
        {
            var copy = self
            if let bold {
                copy.bold = bold
            }
            if let italic {
                copy.italic = italic
            }
            if let strike {
                copy.strike = strike
            }
            if let link {
                copy.link = link
            }
            return copy
        }
    }

    /// Build an attributed string from a block node's inline children.
    /// `context` defaults to the body font; headings/table headers pass their own.
    static func attributed(_ markup: Markup, theme: MarkdownTheme,
                           context: FontContext? = nil,
                           italic: Bool = false) -> AttributedString
    {
        concat(markup.children, style: Style(),
               context: context ?? .body(theme, italic: italic), theme: theme)
    }

    /// Render a single inline node (the node itself, not just its children).
    static func inline(_ markup: Markup, theme: MarkdownTheme,
                       context: FontContext? = nil,
                       italic: Bool = false, strike: Bool = false) -> AttributedString
    {
        render(markup, style: Style(strike: strike),
               context: context ?? .body(theme, italic: italic), theme: theme)
    }

    /// Recursively collect the visible text of a node (e.g. an image's alt text).
    static func plainText(_ markup: Markup) -> String {
        if let text = markup as? Markdown.Text {
            return text.string
        }
        if let code = markup as? InlineCode {
            return code.code
        }
        return markup.children.map { plainText($0) }.joined()
    }

    private static func concat(_ children: some Sequence<Markup>, style: Style,
                               context: FontContext,
                               theme: MarkdownTheme) -> AttributedString
    {
        var out = AttributedString()
        for child in children {
            out.append(render(child, style: style, context: context, theme: theme))
        }
        return out
    }

    private static func render(_ markup: Markup, style: Style,
                               context: FontContext,
                               theme: MarkdownTheme) -> AttributedString
    {
        switch markup {
        case let text as Markdown.Text:
            return AttributedString(text.string, attributes: attributes(style, context, code: false))
        case let code as InlineCode:
            return AttributedString(code.code, attributes: attributes(style, context, code: true))
        case is LineBreak:
            return AttributedString("\n")
        case is SoftBreak:
            return AttributedString(" ")
        case is InlineHTML:
            return AttributedString("")
        case let image as Markdown.Image:
            // Inline images are rendered as blocks elsewhere; here fall back to
            // the alt text so nothing is silently dropped from running text.
            let alt = plainText(image)
            return alt.isEmpty
                ? AttributedString("")
                : AttributedString(alt, attributes: attributes(style, context, code: false))
        default:
            return container(markup, style: style, context: context, theme: theme)
        }
    }

    /// Container nodes (emphasis, strong, strikethrough, links, and anything
    /// unrecognized): layer their styling onto `style`, then render children.
    private static func container(_ markup: Markup, style: Style,
                                  context: FontContext,
                                  theme: MarkdownTheme) -> AttributedString
    {
        let restyled: Style = switch markup {
        case is Emphasis: style.with(italic: true)
        case is Strong: style.with(bold: true)
        case is Strikethrough: style.with(strike: true)
        case let link as Markdown.Link:
            style.with(link: link.destination.flatMap(URL.init(string:)))
        default: style
        }
        return concat(markup.children, style: restyled, context: context, theme: theme)
    }

    private static func attributes(_ style: Style, _ ctx: FontContext,
                                   code: Bool) -> AttributeContainer
    {
        var attrs = AttributeContainer()
        // Start from the context's base font (body size, or a heading's size and
        // weight) then layer inline emphasis on top so nested styles compose.
        attrs.font = (code ? ReaderFont.mono : ctx.font).swiftUIFont(
            size: code ? ctx.size * 0.9 : ctx.size,
            weight: ctx.weight,
            bold: style.bold,
            italic: style.italic || ctx.italic
        )
        // Use explicit Text.LineStyle values so the attribute resolves to the
        // SwiftUI scope (a bare `.single` is ambiguous with Foundation's
        // NSUnderlineStyle).
        if style.strike {
            attrs.strikethroughStyle = Text.LineStyle(pattern: .solid, color: nil)
        }
        if code {
            attrs.backgroundColor = Color.secondary.opacity(0.15)
        }
        if let link = style.link {
            attrs.link = link
            attrs.foregroundColor = .primary
            attrs.underlineStyle = Text.LineStyle(pattern: .solid, color: nil)
        }
        return attrs
    }
}
