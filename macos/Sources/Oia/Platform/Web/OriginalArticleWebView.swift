// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import WebKit

/// Uses WKWebView on the macOS 15 baseline. The page owns the instance so a
/// Saved/Original round trip preserves the web page's scroll position and state.
struct OriginalArticleWebView: NSViewRepresentable {
    let page: OriginalArticlePage

    func makeNSView(context _: Context) -> WKWebView {
        page.webView.setAllMediaPlaybackSuspended(false, completionHandler: nil)
        return page.webView
    }

    func updateNSView(_: WKWebView, context _: Context) {}

    static func dismantleNSView(_ webView: WKWebView, coordinator _: ()) {
        webView.setAllMediaPlaybackSuspended(true, completionHandler: nil)
        if let responder = webView.window?.firstResponder as? NSView,
           responder === webView || responder.isDescendant(of: webView)
        {
            webView.window?.makeFirstResponder(nil)
        }
    }
}
