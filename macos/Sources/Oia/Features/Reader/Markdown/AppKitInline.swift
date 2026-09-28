// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Markdown
import SwiftUI

/// Flattens swift-markdown inline nodes into an `NSAttributedString` with AppKit
/// fonts and colors. This mirrors `InlineRenderer` (which targets SwiftUI's
/// `Text`), but emits `NSFont`/`NSColor` directly because SwiftUI's opaque
/// `Font` does not bridge reliably into an `NSAttributedString` — headings and
/// emphasis would silently fall back to the default system font. Used by the
/// `NSTextView`-backed reader runs (see `SelectableTextView`).
enum AppKitInline {
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

    /// The base run a block sets for its inlines — font metrics and text color.
    /// Fixed for the whole traversal, unlike `Style`, which containers layer as
    /// they nest.
    private struct Run {
        let size: CGFloat
        let weight: Font.Weight
        let design: Font.Design
        let color: NSColor
    }

    /// Build an attributed string from a block node's inline children. `weight`
    /// and `size` set the base run; emphasis (bold/italic/code) layers on top.
    static func attributed(_ markup: Markup, size: CGFloat, weight: Font.Weight,
                           design: Font.Design, color: NSColor = .labelColor,
                           italic: Bool = false) -> NSAttributedString
    {
        let run = Run(size: size, weight: weight, design: design, color: color)
        let out = NSMutableAttributedString()
        for child in markup.children {
            out.append(render(child, style: Style(italic: italic), run: run))
        }
        return out
    }

    private static func render(_ markup: Markup, style: Style, run: Run) -> NSAttributedString {
        switch markup {
        case let text as Markdown.Text:
            return leaf(text.string, style: style, run: run, code: false)
        case let code as InlineCode:
            return leaf(code.code, style: style, run: run, code: true)
        case is LineBreak:
            return NSAttributedString(string: "\n")
        case is SoftBreak:
            return NSAttributedString(string: " ")
        case is InlineHTML:
            return NSAttributedString(string: "")
        case let image as Markdown.Image:
            // Inline images fall back to alt text so nothing is dropped; block
            // images are handled by the SwiftUI renderer outside the text run.
            let alt = InlineRenderer.plainText(image)
            return alt.isEmpty ? NSAttributedString() : leaf(alt, style: style, run: run, code: false)
        default:
            return container(markup, style: style, run: run)
        }
    }

    /// Container nodes (emphasis, strong, strikethrough, links, and anything
    /// unrecognized): layer their styling onto `style`, then render children.
    private static func container(_ markup: Markup, style: Style, run: Run) -> NSAttributedString {
        let restyled: Style = switch markup {
        case is Emphasis: style.with(italic: true)
        case is Strong: style.with(bold: true)
        case is Strikethrough: style.with(strike: true)
        case let link as Markdown.Link:
            style.with(link: link.destination.flatMap(URL.init(string:)))
        default: style
        }
        return concat(markup.children, style: restyled, run: run)
    }

    private static func concat(_ children: some Sequence<Markup>, style: Style, run: Run) -> NSAttributedString {
        let out = NSMutableAttributedString()
        for child in children {
            out.append(render(child, style: style, run: run))
        }
        return out
    }

    private static func leaf(_ string: String, style: Style, run: Run, code: Bool) -> NSAttributedString {
        var attrs: [NSAttributedString.Key: Any] = [:]
        attrs[.font] = makeFont(size: code ? run.size * 0.9 : run.size, weight: run.weight,
                                design: code ? .monospaced : run.design,
                                bold: style.bold, italic: style.italic)
        attrs[.foregroundColor] = run.color
        if style.strike {
            attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        }
        if code {
            attrs[.backgroundColor] = NSColor.secondaryLabelColor.withAlphaComponent(0.15)
        }
        if let link = style.link {
            attrs[.link] = link
            attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue
        }
        return NSAttributedString(string: string, attributes: attrs)
    }

    /// Use an explicit Palatino face for Serif. Other choices retain their system
    /// designs and symbolic traits so nested emphasis composes.
    static func makeFont(size: CGFloat, weight: Font.Weight, design: Font.Design,
                         bold: Bool, italic: Bool) -> NSFont
    {
        if case .serif = design,
           let serif = NSFont(name: ReaderFont.serifFaceName(weight: weight, bold: bold, italic: italic),
                                size: size)
        {
            return serif
        }
        let fallback = NSFont.systemFont(ofSize: size, weight: weight.nsWeight)
        var descriptor = fallback.fontDescriptor
        if let designed = descriptor.withDesign(design.systemDesign) {
            descriptor = designed
        }
        var traits = descriptor.symbolicTraits
        if bold {
            traits.insert(.bold)
        }
        if italic {
            traits.insert(.italic)
        }
        descriptor = descriptor.withSymbolicTraits(traits)
        return NSFont(descriptor: descriptor, size: size) ?? fallback
    }
}

// ── SwiftUI → AppKit mappings ────────────────────────────────────────────────

extension Font.Weight {
    /// Map the SwiftUI weights the theme uses onto AppKit font weights.
    var nsWeight: NSFont.Weight {
        if self == .ultraLight {
            return .ultraLight
        }
        if self == .thin {
            return .thin
        }
        if self == .light {
            return .light
        }
        if self == .medium {
            return .medium
        }
        if self == .semibold {
            return .semibold
        }
        if self == .bold {
            return .bold
        }
        if self == .heavy {
            return .heavy
        }
        if self == .black {
            return .black
        }
        return .regular
    }
}

extension Font.Design {
    var systemDesign: NSFontDescriptor.SystemDesign {
        switch self {
        case .serif: .serif
        case .rounded: .rounded
        case .monospaced: .monospaced
        default: .default
        }
    }
}
