// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Header above the reader's own scroll: the article title and a read-only tag summary.
/// Driven entirely by the passed-in row, so it lives apart from
/// `ArticleDetailView`'s loading state.
struct ArticleHeaderView: View {
    let row: ReadingRow
    /// Reader typography, so the title and tags rescale with the body copy.
    let theme: MarkdownTheme

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(row.title.isEmpty ? "Untitled" : row.title)
                .font(theme.titleFont)
                .tracking(theme.titleTracking)
                .accessibilityIdentifier(A11y.Detail.title)
            if !row.tags.isEmpty {
                Text(row.tags.map { "#\($0)" }.joined(separator: " "))
                    .font(theme.metadataFont)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                    .accessibilityIdentifier(A11y.Detail.tags)
            }
        }
        .padding(.top, 24)
        .padding(.bottom, 16)
        // Same measure as the body below it, so the title stays flush with the
        // article text at every reader width.
        .frame(maxWidth: theme.contentMaxWidth, alignment: .leading)
        .frame(maxWidth: .infinity, alignment: .center)
        .padding(.horizontal, 20)
    }
}
