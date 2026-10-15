// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI
import XCTest

final class OriginalArticleViewTests: XCTestCase {
    @MainActor
    func testNavigatingReplacesTheMountedWebPage() throws {
        let first = try OriginalArticlePage(url: XCTUnwrap(URL(string: "about:blank#first")))
        let second = try OriginalArticlePage(url: XCTUnwrap(URL(string: "about:blank#second")))
        defer {
            first.stopLoading()
            second.stopLoading()
        }
        let host = NSHostingView(rootView: OriginalArticleView(page: first))
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 800, height: 600),
            styleMask: [.titled], backing: .buffered, defer: false
        )
        window.contentView = host
        defer { window.contentView = nil }
        host.layoutSubtreeIfNeeded()
        XCTAssertTrue(first.webView.isDescendant(of: host), "The first page must be mounted initially")

        host.rootView = OriginalArticleView(page: second)
        host.layoutSubtreeIfNeeded()

        XCTAssertTrue(second.webView.isDescendant(of: host), "Navigation must mount the selected article's page")
        XCTAssertFalse(first.webView.isDescendant(of: host), "Navigation must remove the previous article's page")

        host.rootView = OriginalArticleView(page: second)
        host.layoutSubtreeIfNeeded()
        XCTAssertTrue(second.webView.isDescendant(of: host), "Refreshing the same article must retain its page")
    }
}
