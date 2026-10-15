// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Observation
import WebKit

/// Session-only browsing state. It never changes a reading or the library index.
@MainActor
@Observable
final class OriginalArticlePage: NSObject, WKNavigationDelegate {
    let url: URL
    private(set) var isLoading = false
    private(set) var failure: String?
    @ObservationIgnored let webView: WKWebView

    init(url: URL) {
        self.url = url
        webView = WKWebView(frame: .zero)
        super.init()
        webView.navigationDelegate = self
        webView.setAccessibilityLabel("Original article")
        load()
    }

    func load() {
        failure = nil
        isLoading = true
        webView.load(URLRequest(url: url))
    }

    func stopLoading() {
        webView.stopLoading()
    }

    func webView(_: WKWebView, didStartProvisionalNavigation _: WKNavigation!) {
        failure = nil
        isLoading = true
    }

    func webView(_: WKWebView, didFinish _: WKNavigation!) {
        isLoading = false
    }

    func webView(
        _: WKWebView, didFailProvisionalNavigation _: WKNavigation!,
        withError error: any Error
    ) {
        didFail(error)
    }

    func webView(
        _: WKWebView, didFail _: WKNavigation!, withError error: any Error
    ) {
        didFail(error)
    }

    func webViewWebContentProcessDidTerminate(_: WKWebView) {
        isLoading = false
        failure = "The web page stopped responding. Try loading it again."
    }

    func webView(
        _: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
    ) async -> WKNavigationActionPolicy {
        let destination = navigationAction.request.url
        switch OriginalArticleNavigation.decide(
            destination: destination, currentURL: webView.url,
            isLink: navigationAction.navigationType == .linkActivated,
            targetsMainFrame: navigationAction.targetFrame?.isMainFrame
        ) {
        case .allow:
            return .allow
        case .openInBrowser:
            if let destination {
                NSWorkspace.shared.open(destination)
            }
            return .cancel
        case .cancel:
            return .cancel
        }
    }

    private func didFail(_ error: any Error) {
        isLoading = false
        let error = error as NSError
        guard error.domain != NSURLErrorDomain || error.code != NSURLErrorCancelled else { return }
        failure = error.localizedDescription
    }
}
