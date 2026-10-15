// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct OriginalArticleView: View {
    let page: OriginalArticlePage

    var body: some View {
        ZStack {
            OriginalArticleWebView(page: page)
                .opacity(page.failure == nil ? 1 : 0)
                .accessibilityHidden(page.failure != nil)

            if let failure = page.failure {
                ContentUnavailableView {
                    Label("Couldn’t Load Original", systemImage: "network.slash")
                } description: {
                    Text(failure)
                } actions: {
                    Button("Try Again") { page.load() }
                    Link("Open in Browser", destination: page.url)
                }
            } else if page.isLoading {
                ProgressView("Loading original")
                    .controlSize(.small)
                    .padding(16)
                    .background(.regularMaterial, in: .rect(cornerRadius: 8))
                    .allowsHitTesting(false)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityIdentifier(A11y.Detail.originalArticle)
    }
}
