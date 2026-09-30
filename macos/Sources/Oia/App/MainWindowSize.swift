// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// The main window's size is a per-device preference. SwiftUI's implicit frame
/// autosave name includes the WindowGroup's view type, so changing the view tree
/// can make a previously saved size unreachable.
enum MainWindowSize {
    static let defaultSize = CGSize(width: 1100, height: 720)
    static let minimumSize = CGSize(width: 900, height: 600)

    private static let key = "mainWindowContentSize"

    static var saved: CGSize? {
        guard let dimensions = AppDefaults.store.array(forKey: key) as? [Double],
              dimensions.count == 2,
              dimensions.allSatisfy(\.isFinite),
              dimensions[0] >= minimumSize.width,
              dimensions[1] >= minimumSize.height
        else { return nil }

        return CGSize(width: dimensions[0], height: dimensions[1])
    }

    static func save(_ size: CGSize) {
        guard size.width.isFinite, size.height.isFinite,
              size.width >= minimumSize.width,
              size.height >= minimumSize.height
        else { return }

        AppDefaults.store.set([Double(size.width), Double(size.height)], forKey: key)
    }
}

/// Attaches to the actual main NSWindow, including when the window is closed
/// and opened again without quitting. Settings windows never pass through here.
struct MainWindowSizeObserver: NSViewRepresentable {
    func makeNSView(context: Context) -> WindowView { WindowView(frame: .zero) }

    func updateNSView(_ nsView: WindowView, context: Context) {}

    final class WindowView: NSView {
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            NotificationCenter.default.removeObserver(
                self,
                name: NSWindow.didResizeNotification,
                object: nil
            )

            guard let window else { return }
            if let size = MainWindowSize.saved {
                window.setContentSize(size)
            }
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(saveWindowSize),
                name: NSWindow.didResizeNotification,
                object: window
            )
        }

        @objc private func saveWindowSize() {
            guard let window else { return }
            MainWindowSize.save(window.contentRect(forFrameRect: window.frame).size)
        }
    }
}
