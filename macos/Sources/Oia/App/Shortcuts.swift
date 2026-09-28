// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// One physical key binding for an app action. Modified bindings are installed
/// on menu commands; unmodified alternatives stay local to the focused board so
/// they never steal typing from a text field or a confirmation dialog.
struct AppShortcutKey {
    let key: KeyEquivalent
    let modifiers: EventModifiers
    let keyGlyph: String

    /// macOS-style rendering, e.g. "⌘⇧T". Modifier order matches the menu bar:
    /// Control, Option, Shift, Command.
    var display: String {
        var glyphs = ""
        if modifiers.contains(.control) {
            glyphs += "⌃"
        }
        if modifiers.contains(.option) {
            glyphs += "⌥"
        }
        if modifiers.contains(.shift) {
            glyphs += "⇧"
        }
        if modifiers.contains(.command) {
            glyphs += "⌘"
        }
        return glyphs + keyGlyph
    }

    func matches(key: KeyEquivalent, modifiers: EventModifiers) -> Bool {
        let significantModifiers: EventModifiers = [.command, .control, .option, .shift]
        return self.key == key
            && self.modifiers == modifiers.intersection(significantModifiers)
    }
}

/// A shortcut action and every supported key binding. The primary binding is
/// used by `.keyboardShortcut`; focus-local alternatives are interpreted by the
/// board. The cheat sheet reads both, keeping discoverability in sync.
struct AppShortcut: Identifiable {
    let title: String
    let primary: AppShortcutKey
    let alternatives: [AppShortcutKey]

    init(
        title: String,
        key: KeyEquivalent,
        modifiers: EventModifiers,
        keyGlyph: String,
        alternatives: [AppShortcutKey] = []
    ) {
        self.title = title
        primary = AppShortcutKey(key: key, modifiers: modifiers, keyGlyph: keyGlyph)
        self.alternatives = alternatives
    }

    var id: String {
        title
    }

    var display: String {
        primary.display
    }

    var displays: [String] {
        ([primary] + alternatives).map(\.display)
    }

    func matches(key: KeyEquivalent, modifiers: EventModifiers) -> Bool {
        ([primary] + alternatives).contains { binding in
            binding.matches(key: key, modifiers: modifiers)
        }
    }
}

extension View {
    /// Apply a catalog shortcut, so call sites stay in sync with the cheat sheet.
    func keyboardShortcut(_ shortcut: AppShortcut) -> some View {
        keyboardShortcut(shortcut.primary.key, modifiers: shortcut.primary.modifiers)
    }
}

/// Every keyboard shortcut in the app, grouped for display. Adding or changing a
/// shortcut here updates both the menu binding and the cheat sheet at once.
enum ShortcutCatalog {
    struct Group: Identifiable {
        let name: String
        let shortcuts: [AppShortcut]
        var id: String {
            name
        }
    }

    // The catalog below lists one shortcut per row — title / key / modifiers /
    // glyph — so they read top-to-bottom like a table. The rows run long, so
    // line_length is off for this block only.
    // swiftlint:disable line_length

    // Item actions
    static let openWithReturn = AppShortcutKey(key: .return, modifiers: [], keyGlyph: "↩")
    static let open = AppShortcut(
        title: "Open", key: "o", modifiers: .command, keyGlyph: "O",
        alternatives: [openWithReturn]
    )
    static let quickLook = AppShortcut(title: "Quick Look", key: .space, modifiers: [], keyGlyph: "Space")
    static let editTags = AppShortcut(title: "Edit Tags", key: "t", modifiers: [.command, .shift], keyGlyph: "T")
    static let toggleHighlights = AppShortcut(title: "Show / Hide Highlights", key: "h", modifiers: [.command, .shift], keyGlyph: "H")
    static let delete = AppShortcut(title: "Delete", key: .delete, modifiers: .command, keyGlyph: "⌫")
    static let openInBrowser = AppShortcut(title: "Open in Browser", key: "o", modifiers: [.command, .shift], keyGlyph: "O")

    // View / navigation
    static let focusSearchWithSlash = AppShortcutKey(key: "/", modifiers: [], keyGlyph: "/")
    static let focusSearch = AppShortcut(
        title: "Focus Search", key: "f", modifiers: .command, keyGlyph: "F",
        alternatives: [focusSearchWithSlash]
    )
    static let toggleFocusMode = AppShortcut(title: "Toggle Focus Mode", key: "r", modifiers: [.command, .shift], keyGlyph: "R")
    static let scrollToTop = AppShortcut(title: "Scroll to Top", key: .upArrow, modifiers: .command, keyGlyph: "↑")
    static let decreaseCardSize = AppShortcut(title: "Decrease Card Size", key: "-", modifiers: [.command, .shift], keyGlyph: "-")
    static let increaseCardSize = AppShortcut(title: "Increase Card Size", key: "=", modifiers: [.command, .shift], keyGlyph: "+")
    static let showAll = AppShortcut(title: "Show All", key: "1", modifiers: .command, keyGlyph: "1")
    static let showImages = AppShortcut(title: "Show Images", key: "2", modifiers: .command, keyGlyph: "2")
    static let showVideos = AppShortcut(title: "Show Videos", key: "3", modifiers: .command, keyGlyph: "3")
    static let showArticles = AppShortcut(title: "Show Articles", key: "4", modifiers: .command, keyGlyph: "4")
    static let showLinks = AppShortcut(title: "Show Links", key: "5", modifiers: .command, keyGlyph: "5")
    static let showQuotes = AppShortcut(title: "Show Quotes", key: "6", modifiers: .command, keyGlyph: "6")
    static let previousFilter = AppShortcut(title: "Previous Filter", key: "[", modifiers: .command, keyGlyph: "[")
    static let nextFilter = AppShortcut(title: "Next Filter", key: "]", modifiers: .command, keyGlyph: "]")
    static let previousItem = AppShortcut(title: "Previous Item", key: "k", modifiers: [], keyGlyph: "K")
    static let nextItem = AppShortcut(title: "Next Item", key: "j", modifiers: [], keyGlyph: "J")

    // Typography
    static let increaseFont = AppShortcut(title: "Increase Text Size", key: "+", modifiers: .command, keyGlyph: "+")
    static let decreaseFont = AppShortcut(title: "Decrease Text Size", key: "-", modifiers: .command, keyGlyph: "-")

    /// Help
    static let showShortcuts = AppShortcut(title: "Keyboard Shortcuts", key: "/", modifiers: .command, keyGlyph: "/")

    /// Groups in cheat-sheet display order.
    static let groups: [Group] = [
        Group(name: "Item", shortcuts: [open, quickLook, editTags, toggleHighlights, delete, openInBrowser]),
        Group(
            name: "View",
            shortcuts: [
                focusSearch, showAll, showImages, showVideos, showArticles, showLinks, showQuotes,
                previousFilter, nextFilter, toggleFocusMode, decreaseCardSize, increaseCardSize,
                previousItem, nextItem, scrollToTop
            ]
        ),
        Group(name: "Typography", shortcuts: [increaseFont, decreaseFont]),
        Group(name: "Help", shortcuts: [showShortcuts])
    ]

    static func filterShortcut(for scope: LibraryScope) -> AppShortcut {
        switch scope {
        case .all: showAll
        case .images: showImages
        case .videos: showVideos
        case .articles: showArticles
        case .links: showLinks
        case .quotes: showQuotes
        }
    }
    // swiftlint:enable line_length
}
