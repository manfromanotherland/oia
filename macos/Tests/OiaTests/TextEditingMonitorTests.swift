// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Testing
import WebKit

@MainActor
struct TextEditingMonitorTests {
    @Test("Native editable text keeps its editing shortcuts")
    func nativeTextEditing() {
        let textView = NSTextView()
        textView.isEditable = true

        let context = TextEditingMonitor.keyboardContext(for: textView)

        #expect(context.isEditingText)
        #expect(!context.isBrowsingWebContent)
    }

    @Test("Saved reader selection keeps gallery keyboard navigation")
    func readOnlyTextSelection() {
        let textView = NSTextView()
        textView.isEditable = false
        textView.isSelectable = true

        let context = TextEditingMonitor.keyboardContext(for: textView)

        #expect(!context.isEditingText)
        #expect(!context.isBrowsingWebContent)
    }

    @Test("WebKit descendants keep page keys without disabling toolbar buttons")
    func webContentFocus() {
        let webView = WKWebView()
        let content = NSView()
        let input = NSTextView()
        input.isEditable = true
        webView.addSubview(content)
        content.addSubview(input)

        for responder in [webView, content, input] as [NSResponder] {
            let context = TextEditingMonitor.keyboardContext(for: responder)
            #expect(context.isBrowsingWebContent)
            #expect(!context.isEditingText)
        }
    }

    @Test("Leaving web content restores gallery shortcut context")
    func detachedContentFocus() {
        let webView = WKWebView()
        let content = NSView()
        webView.addSubview(content)
        #expect(TextEditingMonitor.keyboardContext(for: content).isBrowsingWebContent)

        content.removeFromSuperview()

        #expect(TextEditingMonitor.keyboardContext(for: content) == .init())
        #expect(TextEditingMonitor.keyboardContext(for: nil) == .init())
    }
}
