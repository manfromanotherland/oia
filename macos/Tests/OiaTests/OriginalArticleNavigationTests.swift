// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import Testing

struct OriginalArticleNavigationTests {
    private let current = URL(string: "https://example.com/article?edition=1#intro")

    @Test("HTTP and HTTPS page loads and redirects stay embedded", arguments: [
        "http://example.com/article", "https://example.com/redirected"
    ])
    func pageLoad(address: String) {
        #expect(decide(address, isLink: false) == .allow)
    }

    @Test("Footnotes and table-of-contents links scroll the current document")
    func sameDocumentFragment() {
        #expect(decide("https://example.com/article?edition=1#footnote") == .allow)
        #expect(decide("https://example.com/article?edition=2#footnote") == .openInBrowser)
        #expect(decide("https://example.com/other#footnote") == .openInBrowser)
    }

    @Test("Following another page opens the browser", arguments: [
        "https://other.example/article", "http://example.com/another", "mailto:author@example.com"
    ])
    func externalLink(address: String) {
        #expect(decide(address) == .openInBrowser)
    }

    @Test("Only user-followed links can open a new window")
    func newWindows() {
        #expect(decide("https://example.com/popup", isLink: false, mainFrame: nil) == .cancel)
        #expect(decide("https://example.com/followed", mainFrame: nil) == .openInBrowser)
    }

    @Test("Local files, private identities and invalid web origins cannot navigate", arguments: [
        "file:///tmp/article.html", "cuttings://local/example", "javascript:alert(1)", "https:invalid"
    ])
    func unsupportedDestination(address: String) {
        #expect(decide(address) == .cancel)
        #expect(decide(address, isLink: false) == .cancel)
    }

    @Test("Embedded frames remain usable without leaving the article")
    func subframes() {
        #expect(decide("https://example.com/embed", mainFrame: false) == .allow)
        #expect(decide("about:blank", isLink: false, mainFrame: false) == .allow)
    }

    private func decide(
        _ address: String, isLink: Bool = true, mainFrame: Bool? = true
    ) -> OriginalArticleNavigation.Decision {
        OriginalArticleNavigation.decide(
            destination: URL(string: address), currentURL: current,
            isLink: isLink, targetsMainFrame: mainFrame
        )
    }
}
