// SPDX-License-Identifier: GPL-3.0-or-later

import Markdown
import SwiftUI

/// `Text` collides between SwiftUI and Markdown; alias the bare name to SwiftUI
/// so paragraph runs resolve. Markdown's node is used as `Markdown.Text`.
private typealias Text = SwiftUI.Text

/// A paragraph is split into runs of inline text and standalone images. The
/// common Substack/Medium pattern `[![alt](img)](url)` (a link wrapping a single
/// image) is detected and rendered as an image, replacing the old
/// `unwrapLinkedImages` regex hack.
struct ParagraphView: View {
    let paragraph: Paragraph
    let theme: MarkdownTheme
    let assetBaseURL: URL?
    /// Block quotes set this so each inline run receives an italic font trait;
    /// a view-level `.italic()` cannot override the renderer's explicit fonts.
    var quoted = false
    /// Completed task items strike their text without affecting nested blocks.
    var struck = false
    var highlights: [String] = []
    var onHighlight: (String) -> Void = { _ in }

    var body: some View {
        VStack(alignment: .leading, spacing: theme.blockSpacing * 0.6) {
            ForEach(segments) { identified in
                switch identified.segment {
                case let .text(attributed):
                    Text(attributed)
                        .lineSpacing(theme.lineSpacing)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                case let .image(source, alt):
                    AssetImageView(source: source, alt: alt, assetBaseURL: assetBaseURL, theme: theme,
                                   highlights: highlights, onHighlight: onHighlight)
                }
            }
        }
    }

    private enum Segment {
        case text(AttributedString)
        case image(source: String, alt: String)
    }

    /// A segment tagged with a source-derived id (the child it began at), so a
    /// paragraph's runs and images keep stable identity instead of keying on
    /// their position in the segment array.
    private struct IdentifiedSegment: Identifiable {
        let id: String
        let segment: Segment
    }

    private var segments: [IdentifiedSegment] {
        var result: [IdentifiedSegment] = []
        var run = AttributedString()
        var runID: String?

        func flush() {
            guard !run.characters.isEmpty else { return }
            result.append(IdentifiedSegment(id: runID ?? "#\(result.count)", segment: .text(run)))
            run = AttributedString()
            runID = nil
        }

        for (offset, child) in paragraph.children.enumerated() {
            if let image = standaloneImage(child) {
                flush()
                result.append(IdentifiedSegment(
                    id: IdentifiedMarkup.stableID(for: child, fallbackIndex: offset),
                    segment: .image(source: image.source ?? "", alt: InlineRenderer.plainText(image))
                ))
            } else {
                // The run inherits the id of the first child that feeds it.
                if run.characters.isEmpty {
                    runID = IdentifiedMarkup.stableID(for: child, fallbackIndex: offset)
                }
                run.append(InlineRenderer.inline(child, theme: theme,
                                                 italic: quoted, strike: struck))
            }
        }
        flush()
        return result.isEmpty
            ? [IdentifiedSegment(id: "#0", segment: .text(AttributedString()))]
            : result
    }

    /// An image either bare, or as the sole meaningful child of a link.
    private func standaloneImage(_ markup: Markup) -> Markdown.Image? {
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
}
