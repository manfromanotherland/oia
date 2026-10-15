// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Keeps the local reader mounted so switching to the source preserves its place.
/// The preferred view follows navigation; each original page is loaded on demand.
struct OiaArticleDetailView: View {
    let row: ReadingRow

    @AppStorage("detailArticleShowsOriginal", store: AppDefaults.store)
    private var prefersOriginal = false
    @State private var originalPage: OriginalArticlePage?

    var body: some View {
        ZStack {
            savedArticle
                .opacity(showsOriginal ? 0 : 1)
                .allowsHitTesting(!showsOriginal)
                .accessibilityHidden(showsOriginal)

            if showsOriginal, let originalPage, originalPage.url == originalURL {
                OriginalArticleView(page: originalPage)
            }
        }
        .toolbar {
            if originalURL != nil {
                ToolbarItem(placement: .primaryAction) {
                    Picker("Article View", selection: $prefersOriginal) {
                        Text("Saved").tag(false)
                        Text("Original").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .fixedSize()
                    .help(
                        "Saved (\(ShortcutCatalog.selectFirstView.display)) or "
                            + "Original (\(ShortcutCatalog.selectOriginalView.display))"
                    )
                    .accessibilityIdentifier(A11y.Detail.articleView)
                }
            }
        }
        .focusedSceneValue(\.detailViewActions, DetailViewActions(
            firstTitle: "Saved", originalTitle: "Original",
            canSelectFirst: true, canSelectOriginal: originalURL != nil,
            canAdjustTypography: !showsOriginal,
            selectFirst: { prefersOriginal = false },
            selectOriginal: {
                if originalURL != nil {
                    prefersOriginal = true
                }
            }
        ))
        .onChange(of: row.id) { _, _ in
            originalPage?.stopLoading()
            originalPage = nil
            loadOriginalIfNeeded()
        }
        .onChange(of: prefersOriginal) { _, _ in
            loadOriginalIfNeeded()
        }
        .onAppear(perform: loadOriginalIfNeeded)
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

    private var showsOriginal: Bool {
        prefersOriginal && originalURL != nil
    }

    private func loadOriginalIfNeeded() {
        guard showsOriginal, originalPage == nil, let url = originalURL else { return }
        originalPage = OriginalArticlePage(url: url)
    }

    private var originalURL: URL? {
        guard let url = row.sourceURL, url.host != nil else { return nil }
        return url
    }
}
