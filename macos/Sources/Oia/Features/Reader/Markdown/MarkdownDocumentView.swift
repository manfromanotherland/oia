// SPDX-License-Identifier: GPL-3.0-or-later

import Markdown
import SwiftUI

/// The parsed block structure of an article, computed once from its Markdown.
/// Parsing (`Document(parsing:)` + grouping) is the expensive step of rendering,
/// so callers build this **off the per-render path** — when the article loads —
/// and hand it to `MarkdownDocumentView`. The view then only applies the theme
/// and highlight tinting on each render, never re-parsing. (Previously the parse
/// lived in the view's initializer, so every unrelated re-render of the reader —
/// a highlight toggle, a font change, a selection advance — re-parsed the whole
/// article on the main thread.)
struct ArticleDocument {
    /// A render unit: either a run of contiguous text blocks — headings,
    /// paragraphs, and image-free lists and quotes — rendered as one selectable
    /// `NSTextView`, or a single non-foldable block (a figure, a table, a code
    /// block, or an image-bearing list/quote) rendered by the SwiftUI block
    /// renderer. Selection is continuous *within* a text run; the non-foldable
    /// blocks form a seam.
    enum RenderGroup: Identifiable {
        case textRun(id: String, blocks: [Markup])
        case other(IdentifiedMarkup)

        var id: String {
            switch self {
            case let .textRun(id, _): id
            case let .other(item): item.id
            }
        }
    }

    let groups: [RenderGroup]

    init(markdown: String) {
        let document = Document(parsing: Self.unwrapLinkedImages(markdown))
        groups = Self.makeGroups(Array(document.children))
    }

    /// Group the document's top-level blocks, merging maximal runs of foldable
    /// blocks (see `isFoldable`). A run takes the source-derived id of its first block,
    /// and each standalone block its own — both distinct source positions, so
    /// ids stay unique across the group list.
    private static func makeGroups(_ blocks: [Markup]) -> [RenderGroup] {
        var groups: [RenderGroup] = []
        var run: [Markup] = []
        var runStart = 0

        func flush() {
            guard let first = run.first else { return }
            groups.append(.textRun(id: IdentifiedMarkup.stableID(for: first, fallbackIndex: runStart),
                                   blocks: run))
            run = []
        }

        for (offset, block) in blocks.enumerated() {
            if isFoldable(block) {
                if run.isEmpty {
                    runStart = offset
                }
                run.append(block)
            } else {
                flush()
                groups.append(.other(IdentifiedMarkup(
                    id: IdentifiedMarkup.stableID(for: block, fallbackIndex: offset), markup: block
                )))
            }
        }
        flush()
        return groups
    }

    /// Which blocks fold into a shared selectable text run, so a drag-selection
    /// flows continuously across them. `MarkdownTextRun` emits headings,
    /// paragraphs, lists, and quotes into one `NSAttributedString`, so all four
    /// can share a run.
    ///
    /// A block is *not* foldable — and so becomes a standalone group and a
    /// selection seam — when it carries an image: a standalone-image paragraph
    /// (a figure), or a list/quote whose subtree contains any image. The text-run
    /// emitter would flatten an embedded image to its alt text, so those render
    /// via the SwiftUI block views instead, which lay the image out as a figure.
    /// Code blocks, tables, and thematic breaks are likewise never foldable.
    private static func isFoldable(_ block: Markup) -> Bool {
        if block is Heading {
            return true
        }
        if let paragraph = block as? Paragraph {
            return !paragraph.children.contains { standaloneImage($0) != nil }
        }
        if block is UnorderedList || block is OrderedList || block is BlockQuote {
            return !containsImage(block)
        }
        return false
    }

    /// True if the block's subtree contains any image. Lists and quotes fold into
    /// a text run only when image-free, since `MarkdownTextRun` would otherwise
    /// flatten the image to alt text.
    private static func containsImage(_ markup: Markup) -> Bool {
        if markup is Markdown.Image {
            return true
        }
        return markup.children.contains { containsImage($0) }
    }

    /// Mirrors `ParagraphView.standaloneImage`: a bare image, or a link wrapping
    /// a single image. Such paragraphs render as figures, not text.
    private static func standaloneImage(_ markup: Markup) -> Markdown.Image? {
        if let image = markup as? Markdown.Image {
            return image
        }
        if let link = markup as? Markdown.Link {
            let children = Array(link.children)
            let images = children.compactMap { $0 as? Markdown.Image }
            let meaningful = children.filter { child in
                if let text = child as? Markdown.Text {
                    return !text.string.trimmingCharacters(in: .whitespaces).isEmpty
                }
                return true
            }
            if images.count == 1, meaningful.count == 1 {
                return images.first
            }
        }
        return nil
    }

    /// Collapse a link wrapping a single image — `[![alt](img)](url)`, often
    /// split across blank lines by the extension's HTML→Markdown step as
    /// `[\n\n![alt](img)\n\n](url)` — down to the bare image. When the wrapper
    /// spans blank lines CommonMark can't parse it as one link, so it would
    /// otherwise leak a literal `[` and `](url)` around the picture. The image
    /// itself is already stored locally; the outer link target is dropped.
    private static func unwrapLinkedImages(_ markdown: String) -> String {
        let pattern = #"\[\s*(!\[[^\]]*\]\([^)]*\))\s*\]\([^)]*\)"#
        return markdown.replacingOccurrences(
            of: pattern, with: "$1", options: .regularExpression
        )
    }
}

extension ArticleDocument {
    /// Parse `markdown` off the calling actor. swift-markdown builds the whole
    /// tree in one synchronous pass — long enough on a large article to stall
    /// the main thread — so callers await this rather than calling
    /// `init(markdown:)` on the main actor.
    static func parse(markdown: String) async -> ArticleDocument {
        let parsed = await Task.detached(priority: .userInitiated) {
            Parsed(document: ArticleDocument(markdown: markdown))
        }.value
        return parsed.document
    }

    /// Lets a document parsed in a detached task cross back to the caller's
    /// actor. Safe because the tree is fully built inside the task and never
    /// mutated afterward — ownership transfers with the box. (swift-markdown's
    /// `Markup` nodes don't declare `Sendable`.)
    private struct Parsed: @unchecked Sendable {
        let document: ArticleDocument
    }
}

/// Native reader: renders a pre-parsed `ArticleDocument` as a SwiftUI view tree.
/// Replaces the `WKWebView`-based `MarkdownWebView`. Light/Dark adapt
/// automatically via semantic colors (appearance is applied app-wide in
/// `OiaApp`), and links open in the system browser.
struct MarkdownDocumentView<Header: View, Footer: View>: View {
    let document: ArticleDocument
    /// The reading's own folder URL, against which its relative asset links
    /// (`assets/<file>`) resolve. See `AssetImageLoader.readingFolderURL`.
    let assetBaseURL: URL?
    var font: ReaderFont = .defaultChoice
    var fontSize: ReaderFontSize = .medium
    var width: ReaderWidth = .medium
    var lineHeight: ReaderLineHeight = .normal
    /// Verbatim text of the reading's highlights; each occurrence is tinted.
    var highlights: [String] = []
    /// Called with the selected text when the user highlights a passage.
    var onHighlight: (String) -> Void = { _ in }
    /// Leading content rendered inside the scroll, before the article body.
    @ViewBuilder var header: () -> Header
    /// Trailing content rendered inside the scroll, after the article body — so
    /// it comes into view only when the reader reaches the end of the article.
    @ViewBuilder var footer: () -> Footer

    init(document: ArticleDocument, assetBaseURL: URL?,
         font: ReaderFont = .defaultChoice, fontSize: ReaderFontSize = .medium,
         width: ReaderWidth = .medium, lineHeight: ReaderLineHeight = .normal,
         highlights: [String] = [], onHighlight: @escaping (String) -> Void = { _ in },
         @ViewBuilder header: @escaping () -> Header = { EmptyView() },
         @ViewBuilder footer: @escaping () -> Footer = { EmptyView() })
    {
        self.document = document
        self.assetBaseURL = assetBaseURL
        self.font = font
        self.fontSize = fontSize
        self.width = width
        self.lineHeight = lineHeight
        self.highlights = highlights
        self.onHighlight = onHighlight
        self.header = header
        self.footer = footer
    }

    private var theme: MarkdownTheme {
        MarkdownTheme(font: font, fontSize: fontSize, width: width, lineHeight: lineHeight)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                header()
                LazyVStack(alignment: .leading, spacing: theme.blockSpacing) {
                    ForEach(document.groups) { group in
                        switch group {
                        case let .textRun(_, blocks):
                            SelectableTextView(
                                attributed: MarkdownTextRun.attributed(blocks, theme: theme),
                                highlights: highlights,
                                onHighlight: onHighlight
                            )
                            .frame(maxWidth: .infinity, alignment: .leading)
                        case let .other(item):
                            MarkdownBlockView(block: item.markup, theme: theme, assetBaseURL: assetBaseURL,
                                              highlights: highlights, onHighlight: onHighlight)
                        }
                    }
                    footer()
                        .frame(maxWidth: .infinity)
                }
                .font(theme.bodyFont)
                .frame(maxWidth: theme.contentMaxWidth, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .center)
                .padding(.horizontal, 20)
                .padding(.top, 24)
                .padding(.bottom, 80)
                // Behind the content: a click in the margins or between blocks clears
                // any active text selection, so clicking outside the text deselects.
                .background(SelectionClearingBackground())
            }
        }
        // SwiftUI can apply the app accent to links inside image-bearing blocks
        // and tables even when their attributed text requests primary color.
        // Keep every reader link in the same neutral ink as the AppKit text runs.
        .tint(.primary)
        .environment(\.openURL, OpenURLAction { url in
            NSWorkspace.shared.open(url)
            return .handled
        })
    }
}
