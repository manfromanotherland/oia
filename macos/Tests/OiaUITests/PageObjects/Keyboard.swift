// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// Drives the app's keyboard shortcuts, mirroring `ShortcutCatalog`. Keeping them
/// in one place means a journey reads in product terms rather than raw `typeKey`
/// calls, and a shortcut change is updated once.
struct Keyboard {
    let app: XCUIApplication

    /// ── Article actions (act on the current selection) ────────────────────
    func open() {
        app.typeKey("o", modifierFlags: .command)
    } // ⌘O
    func openWithReturn() {
        app.typeKey(.return, modifierFlags: [])
    }

    func quickLook() {
        app.typeKey(.space, modifierFlags: [])
    }

    func editTags() {
        app.typeKey("t", modifierFlags: [.command, .shift])
    } // ⌘⇧T
    func toggleHighlights() {
        app.typeKey("h", modifierFlags: [.command, .shift])
    } // ⌘⇧H
    func delete() {
        app.typeKey(.delete, modifierFlags: .command)
    } // ⌘⌫
    func openInBrowser() {
        app.typeKey("o", modifierFlags: [.command, .shift])
    } // ⌘⇧O

    /// ── View / navigation ─────────────────────────────────────────────────
    func focusSearch() {
        app.typeKey("f", modifierFlags: .command)
    } // ⌘F
    func focusSearchWithSlash() {
        app.typeKey("/", modifierFlags: [])
    }

    func toggleFocusMode() {
        app.typeKey("r", modifierFlags: [.command, .shift])
    } // ⌘⇧R

    func toggleSidebar() {
        app.typeKey("b", modifierFlags: .command)
    } // ⌘B

    func previousItem() {
        app.typeKey("k", modifierFlags: [])
    } // K

    func nextItem() {
        app.typeKey("j", modifierFlags: [])
    } // J

    func arrowLeft() {
        app.typeKey(.leftArrow, modifierFlags: [])
    }

    func selectFilter(number: String) {
        app.typeKey(number, modifierFlags: .command)
    }

    func previousFilter() {
        app.typeKey("[", modifierFlags: .command)
    }

    func nextFilter() {
        app.typeKey("]", modifierFlags: .command)
    }

    func arrowRight() {
        app.typeKey(.rightArrow, modifierFlags: [])
    }

    /// ── Reading ────────────────────────────────────────────────────────────
    func increaseFontSize() {
        app.typeKey("+", modifierFlags: .command)
    } // ⌘+
    func decreaseFontSize() {
        app.typeKey("-", modifierFlags: .command)
    } // ⌘-

    /// ── Help ──────────────────────────────────────────────────────────────
    func showShortcuts() {
        app.typeKey("/", modifierFlags: .command)
    } // ⌘/

    /// ── List navigation ─────────────────────────────────────────────────────
    func arrowDown() {
        app.typeKey(.downArrow, modifierFlags: [])
    }

    func arrowUp() {
        app.typeKey(.upArrow, modifierFlags: [])
    }

    /// Escape — clears search / dismisses.
    func escape() {
        app.typeKey(.escape, modifierFlags: [])
    }
}
