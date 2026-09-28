// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import LazyLayoutKit
import SwiftUI

extension OiaLibraryView {
    func select(_ row: ReadingRow, extending: Bool) {
        guard presentedReading == nil else { return }
        appState.selectReading(id: row.id, extending: extending)
        boardFocused = true
    }

    func moveSelection(_ press: KeyPress) -> KeyPress.Result {
        guard !appState.isEditingText, presentedReading == nil else { return .ignored }
        guard press.modifiers.isDisjoint(with: [.command, .control, .option]) else {
            return .ignored
        }
        guard let direction = navigationDirection(for: press.key) else { return .ignored }

        guard let currentID = appState.selectedId,
              appState.readings.contains(where: { $0.id == currentID })
        else {
            guard let firstID = appState.readings.first?.id else { return .handled }
            appState.selectReading(id: firstID, extending: false)
            boardPosition.scrollTo(id: firstID, anchor: .nearest)
            return .handled
        }

        guard let targetID = boardNavigation.neighbor(of: currentID, toward: direction),
              appState.readings.contains(where: { $0.id == targetID })
        else {
            return .handled
        }
        appState.selectReading(
            id: targetID,
            extending: press.modifiers.contains(.shift)
        )
        boardPosition.scrollTo(id: targetID, anchor: .nearest)
        return .handled
    }

    func performBoardShortcut(_ press: KeyPress) -> KeyPress.Result {
        guard !appState.isEditingText, presentedReading == nil else { return .ignored }

        if ShortcutCatalog.copy.matches(key: press.key, modifiers: press.modifiers),
           let row = singleSelectedRow,
           ReadingClipboard.fileURL(for: row, libraryURL: appState.libraryURL) != nil
        {
            ReadingClipboard.copyFile(row, libraryURL: appState.libraryURL)
            return .handled
        }

        if ShortcutCatalog.open.matches(key: press.key, modifiers: press.modifiers) {
            guard appState.selectedRows.count == 1 else { return .ignored }
            openSelection()
            return .handled
        }

        if ShortcutCatalog.quickLook.matches(key: press.key, modifiers: press.modifiers) {
            guard appState.selectedRows.count == 1, appState.libraryURL != nil else {
                return .ignored
            }
            toggleQuickLook()
            return .handled
        }

        if ShortcutCatalog.focusSearch.matches(key: press.key, modifiers: press.modifiers),
           !appState.isFocusMode
        {
            // The menu command and this board-local `/` path share SwiftUI's
            // native `.searchFocused` binding.
            focusSearch()
            return .handled
        }

        return .ignored
    }

    private func navigationDirection(for key: KeyEquivalent) -> BoardNavigationDirection? {
        switch key {
        case .upArrow:
            .upward
        case .downArrow:
            .downward
        case .leftArrow:
            .leftward
        case .rightArrow:
            .rightward
        default:
            nil
        }
    }
}

/// Escape belongs to the board even when a card or the scroll view is the
/// first responder. A local monitor keeps text fields and other windows free
/// to handle their own Escape presses.
struct BoardEscapeMonitor: NSViewRepresentable {
    let onEscape: () -> Bool

    func makeNSView(context: Context) -> EscapeView {
        let view = EscapeView()
        view.onEscape = onEscape
        return view
    }

    func updateNSView(_ view: EscapeView, context: Context) {
        view.onEscape = onEscape
    }

    final class EscapeView: NSView {
        var onEscape: (() -> Bool)?
        private var monitor: Any?

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let monitor {
                NSEvent.removeMonitor(monitor)
                self.monitor = nil
            }
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
                guard let self, event.keyCode == 53, event.window === self.window,
                      self.onEscape?() == true else { return event }
                return nil
            }
        }

    }
}
