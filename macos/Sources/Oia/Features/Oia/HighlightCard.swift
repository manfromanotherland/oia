// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct HighlightCard: View {
    @Environment(\.colorScheme) private var colorScheme
    let text: String
    let cardSize: CardSize

    var body: some View {
        Text(text)
            .font(Font(OiaCardTextMetrics.highlightFont(for: cardSize)))
            .foregroundStyle(.primary)
            .lineSpacing(OiaCardTextMetrics.highlightLineSpacing)
            .multilineTextAlignment(.leading)
            .lineLimit(OiaCardTextMetrics.highlightLineLimit)
            .fixedSize(horizontal: false, vertical: true)
            .textRenderer(HighlightMarkerRenderer(
                color: Color(red: 1, green: 224 / 255, blue: 102 / 255)
                    .opacity(colorScheme == .dark ? 0.28 : 0.55)
            ))
            .padding(OiaCardTextMetrics.highlightPadding)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Draws marker ink behind each wrapped line without changing text layout or
/// turning the whole paragraph into a rectangular yellow panel.
private struct HighlightMarkerRenderer: TextRenderer {
    let color: Color

    func draw(layout: Text.Layout, in context: inout GraphicsContext) {
        for line in layout {
            let bounds = line.typographicBounds.rect
            if bounds.width > 0 {
                context.fill(Path(bounds.insetBy(dx: -2, dy: -1)), with: .color(color))
            }
            context.draw(line)
        }
    }

    var displayPadding: EdgeInsets {
        EdgeInsets(top: 1, leading: 2, bottom: 1, trailing: 2)
    }
}
