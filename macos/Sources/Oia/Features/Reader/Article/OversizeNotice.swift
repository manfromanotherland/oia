// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Shown instead of the reader when a reading's body is too large to parse (see
/// `ArticleDetailView.maxParseBytes` / `maxParseWords`). The full text is still
/// available in the browser.
struct OversizeNotice: View {
    let url: URL?

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: 40))
                .foregroundStyle(.tertiary)
            Text("This article is too large to display in the reader")
                .font(.headline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Text(
                url == nil
                    ? "The full text remains saved in your Óia library."
                    : "Open it in your browser to read the full text."
            )
            .font(.callout)
            .foregroundStyle(.tertiary)
            .multilineTextAlignment(.center)
            if let url {
                Button("Open in Browser") {
                    ReadingLink.open(url)
                }
                .buttonStyle(.bordered)
                .tint(.primary)
                .padding(.top, 4)
                .accessibilityIdentifier(A11y.Detail.oversizeOpenInBrowser)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
        .accessibilityIdentifier(A11y.Detail.oversize)
    }
}
