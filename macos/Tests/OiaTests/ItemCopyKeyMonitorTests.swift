// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Testing
import WebKit

@MainActor
struct ItemCopyKeyMonitorTests {
    @Test("Command-C copies the item before board or web content responders")
    func itemAndWebContentCopy() throws {
        let window = makeWindow()
        defer { window.contentView = nil }
        var copyCount = 0
        let monitor = ItemCopyKeyMonitorView {
            copyCount += 1
            return true
        }
        window.contentView?.addSubview(monitor)
        let event = try copyEvent(in: window)

        #expect(monitor.handle(event) == nil)

        let webView = WKWebView()
        window.contentView?.addSubview(webView)
        #expect(window.makeFirstResponder(webView))
        #expect(monitor.handle(event) == nil)
        #expect(copyCount == 2)
    }

    @Test("Editable fields keep native text copying")
    func nativeTextCopy() throws {
        let window = makeWindow()
        defer { window.contentView = nil }
        var copyCount = 0
        let monitor = ItemCopyKeyMonitorView {
            copyCount += 1
            return true
        }
        window.contentView?.addSubview(monitor)
        let textView = NSTextView()
        textView.isEditable = true
        window.contentView?.addSubview(textView)
        #expect(window.makeFirstResponder(textView))
        let event = try copyEvent(in: window)

        #expect(monitor.handle(event) === event)
        #expect(copyCount == 0)
    }

    @Test("Only plain Command-C is routed and caps lock does not affect it")
    func exactCopyShortcut() throws {
        let window = makeWindow()
        defer { window.contentView = nil }
        var copyCount = 0
        let monitor = ItemCopyKeyMonitorView {
            copyCount += 1
            return true
        }
        window.contentView?.addSubview(monitor)

        let rejectedModifiers: [NSEvent.ModifierFlags] = [
            [], .shift, [.command, .shift], [.command, .option], [.command, .control],
            [.command, .function]
        ]
        for modifiers in rejectedModifiers {
            let event = try copyEvent(in: window, modifiers: modifiers)
            #expect(monitor.handle(event) === event)
        }
        let otherKey = try copyEvent(in: window, characters: "x")
        #expect(monitor.handle(otherKey) === otherKey)
        let capsEvent = try copyEvent(
            in: window, modifiers: [.command, .capsLock, .numericPad], characters: "C"
        )
        #expect(monitor.handle(capsEvent) == nil)
        #expect(copyCount == 1)
    }

    @Test("Unhandled commands and events from other windows pass through")
    func unhandledAndOtherWindows() throws {
        let window = makeWindow()
        let otherWindow = makeWindow()
        defer {
            window.contentView = nil
            otherWindow.contentView = nil
        }
        var copyCount = 0
        let monitor = ItemCopyKeyMonitorView {
            copyCount += 1
            return false
        }
        window.contentView?.addSubview(monitor)
        let ownEvent = try copyEvent(in: window)
        let otherEvent = try copyEvent(in: otherWindow)

        #expect(monitor.handle(otherEvent) === otherEvent)
        #expect(copyCount == 0)
        #expect(monitor.handle(ownEvent) === ownEvent)
        #expect(copyCount == 1)

        monitor.removeFromSuperview()
        #expect(monitor.handle(ownEvent) === ownEvent)
        #expect(copyCount == 1)
    }

    private func makeWindow() -> NSWindow {
        _ = NSApplication.shared
        return NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 300, height: 200),
            styleMask: [.titled], backing: .buffered, defer: false
        )
    }

    private func copyEvent(
        in window: NSWindow,
        modifiers: NSEvent.ModifierFlags = .command,
        characters: String = "c"
    ) throws -> NSEvent {
        try #require(NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0,
            windowNumber: window.windowNumber, context: nil, characters: characters,
            charactersIgnoringModifiers: characters, isARepeat: false, keyCode: 8
        ))
    }
}
