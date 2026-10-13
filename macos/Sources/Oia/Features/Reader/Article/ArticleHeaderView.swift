// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Header above the reader's own scroll: the article title.
/// Driven entirely by the passed-in row, so it lives apart from
/// `ArticleDetailView`'s loading state.
struct ArticleHeaderView: View {
    let row: ReadingRow
    /// Reader typography, so the title rescales with the body copy.
    let theme: MarkdownTheme

    var body: some View {
        Text(row.title.isEmpty ? "Untitled" : row.title)
            .font(theme.titleFont)
            .tracking(theme.titleTracking)
            .multilineTextAlignment(.leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityIdentifier(A11y.Detail.title)
            .padding(.top, 20)
            // Same measure as the body below it, so the title stays flush with the
            // article text at every reader width.
            .frame(maxWidth: theme.contentMaxWidth, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 20)
    }
}
