// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Routes item copying before the native Edit menu or an embedded web page can
/// claim Command-C. Editable native fields retain their normal text copying.
struct ItemCopyKeyMonitor: NSViewRepresentable {
    var onCopy: () -> Bool

    func makeNSView(context _: Context) -> ItemCopyKeyMonitorView {
        ItemCopyKeyMonitorView(onCopy: onCopy)
    }

    func updateNSView(_ nsView: ItemCopyKeyMonitorView, context _: Context) {
        nsView.onCopy = onCopy
    }

    static func dismantleNSView(_ nsView: ItemCopyKeyMonitorView, coordinator _: ()) {
        nsView.stopMonitoring()
    }
}

final class ItemCopyKeyMonitorView: NSView {
    var onCopy: () -> Bool

    /// AppKit delivers local event monitors on the main thread. The token is
    /// otherwise read only during exclusive deinitialization, like our observers.
    private nonisolated(unsafe) var eventMonitor: Any?

    init(onCopy: @escaping () -> Bool) {
        self.onCopy = onCopy
        super.init(frame: .zero)
    }

    required init?(coder _: NSCoder) {
        nil
    }

    override func hitTest(_: NSPoint) -> NSView? {
        nil
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        stopMonitoring()
        guard window != nil else { return }
        eventMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            let handled = MainActor.assumeIsolated {
                guard let self else { return false }
                return self.handle(event) == nil
            }
            return handled ? nil : event
        }
    }

    deinit {
        if let eventMonitor {
            NSEvent.removeMonitor(eventMonitor)
        }
    }

    func stopMonitoring() {
        if let eventMonitor {
            NSEvent.removeMonitor(eventMonitor)
        }
        eventMonitor = nil
    }

    /// The same handler serves the local monitor and synthetic event tests.
    func handle(_ event: NSEvent) -> NSEvent? {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            .subtracting([.capsLock, .numericPad])
        guard event.type == .keyDown,
              let window,
              event.window === window,
              modifiers == .command,
              event.charactersIgnoringModifiers?.lowercased() == "c",
              !TextEditingMonitor.keyboardContext(for: window.firstResponder).isEditingText,
              onCopy()
        else { return event }
        return nil
    }
}
