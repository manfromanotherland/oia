// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Keeps the local reader mounted so switching to the source preserves its place.
/// A web page is created only after the user chooses Original for this reading.
struct OiaArticleDetailView: View {
    let row: ReadingRow

    @State private var showsOriginal = false
    @State private var originalPage: OriginalArticlePage?

    var body: some View {
        ZStack {
            savedArticle
                .opacity(showsOriginal ? 0 : 1)
                .allowsHitTesting(!showsOriginal)
                .accessibilityHidden(showsOriginal)

            if showsOriginal, let originalPage {
                OriginalArticleView(page: originalPage)
            }
        }
        .toolbar {
            if originalURL != nil {
                ToolbarItem(placement: .primaryAction) {
                    Picker("Article View", selection: viewSelection) {
                        Text("Saved").tag(false)
                        Text("Original").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .fixedSize()
                    .help("Read the saved article or view its original web page")
                    .accessibilityIdentifier(A11y.Detail.articleView)
                }
            }
        }
        .onChange(of: row.id) { _, _ in
            showsOriginal = false
            originalPage?.stopLoading()
            originalPage = nil
        }
        .onDisappear {
            originalPage?.stopLoading()
        }
    }

    @ViewBuilder
    private var savedArticle: some View {
        if row.featuredHighlight == nil, let profile = row.socialPostProfile {
            SocialPostDetailView(row: row, profile: profile)
        } else {
            ArticleDetailView(showsToolbar: false)
                .background(Color(nsColor: .textBackgroundColor))
        }
    }

    private var viewSelection: Binding<Bool> {
        Binding(
            get: { showsOriginal },
            set: { selected in
                if selected, originalPage == nil, let url = originalURL {
                    originalPage = OriginalArticlePage(url: url)
                }
                showsOriginal = selected && originalPage != nil
            }
        )
    }

    private var originalURL: URL? {
        guard let url = row.sourceURL, url.host != nil else { return nil }
        return url
    }
}
