// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct OriginalArticleView: View {
    let page: OriginalArticlePage

    var body: some View {
        ZStack {
            OriginalArticleWebView(page: page)
                // A representable's native view is created once per identity.
                // Replacing the page must also replace its owned WKWebView.
                .id(ObjectIdentifier(page))
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
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityIdentifier(A11y.Detail.originalArticle)
    }
}
