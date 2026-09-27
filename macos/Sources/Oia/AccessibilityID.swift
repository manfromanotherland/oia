// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Stable accessibility identifiers shared by the app and the XCUITest suite, so
/// tests locate controls by identifier rather than by (localizable, ambiguous)
/// on-screen wording.
///
/// This file is compiled into **both** the `Óia` app target and the
/// `OiaUITests` target (it lives under `Sources/Oia/`, and the test
/// target lists it explicitly) so the two sides agree on one source of truth.
///
/// **Dependency-free by design:** the UI-test target can't see app types, so the
/// parameterized helpers take primitives (`String`/`Int`/`UInt8`). The app passes
/// `item.id` / `mode.id` / a reading id at the call site; a test passes the same
/// literal (e.g. `A11y.Filter.tag("local-first")`).
enum A11y {
    /// ── Onboarding ────────────────────────────────────────────────────────
    enum Onboarding {
        static let title = "onboarding.title"
        static let chooseLibrary = "onboarding.chooseLibrary"
    }

    /// ── Browser extension ─────────────────────────────────────────────────
    /// Store-listing links in Settings › Extensions.
    enum Extensions {
        static let chromeLink = "extensions.chrome"
        static let firefoxLink = "extensions.firefox"
    }

    /// ── Board filters ─────────────────────────────────────────────────────
    enum Filter {
        static let group = "filter.group"
        static let menu = "filter.menu"
        static let chooseTag = "filter.chooseTag"
        static let tagSheet = "filter.tagSheet"
        static let tagSearch = "filter.tagSearch"

        /// A tag option in the toolbar Filter menu, keyed by tag name.
        static func tag(_ tag: String) -> String {
            "filter.tag.\(tag)"
        }
    }

    /// ── Reading list ──────────────────────────────────────────────────────
    enum List {
        static let table = "list.table"
        /// A reading row, keyed by reading id.
        static func row(_ id: String) -> String {
            "list.row.\(id)"
        }

        /// A hidden probe describing the loaded rows: its accessibility **label**
        /// is the row count and its **value** is the ordered row ids (comma-
        /// joined). The suite reads both from one `firstMatch` instead of
        /// enumerating rows, which trips an XCUITest snapshot bug on article
        /// headings (`AXHeading`).
        static let rows = "list.rows"
        static let cardSizeControl = "list.cardSize"
        /// Default "Nothing here yet" empty state.
        static let emptyState = "list.empty"
        /// Search "No results" empty state.
        static let searchEmptyState = "list.empty.search"
        /// Tag-filter empty state (offers Clear tag filter).
        static let tagEmptyState = "list.empty.tag"
        static let clearTagFilter = "list.clearTagFilter"
    }

    /// ── Paste/drop save feedback ──────────────────────────────────────────
    enum Save {
        static let dropTarget = "save.dropTarget"
        static let notice = "save.notice"
    }

    /// ── Reader / detail ─────────────────────────────────────────────────────
    enum Detail {
        static let title = "detail.title"
        static let tags = "detail.tags"
        static let close = "detail.close"
        static let previous = "detail.previous"
        static let next = "detail.next"
        static let empty = "detail.empty"
        static let oversize = "detail.oversize"
        static let oversizeOpenInBrowser = "detail.oversize.openInBrowser"
        static let videoPlayer = "detail.videoPlayer"
        static let videoUnavailable = "detail.videoUnavailable"
    }

    enum Inspector {
        static let panel = "detail.inspector"
        static let toggle = "detail.inspector.toggle"
        static let tabs = "detail.inspector.tabs"
        static let editTags = "detail.inspector.editTags"
        static let file = "detail.inspector.file"
        static let colorPrefix = "detail.inspector.colour."
        static func attribute(_ label: String) -> String { "detail.inspector.attribute.\(label)" }
    }

    /// ── Reader content ────────────────────────────────────────────────────────
    enum Reader {
        /// A tappable article image (figure). Shared by every figure in the
        /// reader, so tests match the first one.
        static let figure = "reader.figure"
    }

    /// ── Image zoom lightbox ───────────────────────────────────────────────────
    enum Lightbox {
        static let image = "lightbox.image"
        static let close = "lightbox.close"
    }

    /// ── Reader toolbar ────────────────────────────────────────────────────
    enum Toolbar {
        static let openInBrowser = "toolbar.openInBrowser"
        static let tags = "toolbar.tags"
        static let highlight = "toolbar.highlight"
        /// The "select some text first" popover raised by `highlight`.
        static let highlightHint = "toolbar.highlightHint"
        static let delete = "toolbar.delete"
    }

    /// ── Tag picker sheet ────────────────────────────────────────────────────
    enum TagPicker {
        static let searchField = "tagPicker.search"
        static let addRow = "tagPicker.add"
        /// Inline message shown when the typed name exceeds the tag-length limit.
        static let lengthError = "tagPicker.lengthError"
        /// An existing-tag toggle row, keyed by tag name.
        static func row(_ tag: String) -> String {
            "tagPicker.row.\(tag)"
        }

        static let done = "tagPicker.done"
    }

    /// ── Highlights inspector ────────────────────────────────────────────────
    enum Highlights {
        static let list = "highlights.list"
        /// A highlight row, keyed by highlight id.
        static func row(_ id: String) -> String {
            "highlights.row.\(id)"
        }

        static let emptyState = "highlights.empty"
    }

    /// ── Shortcuts sheet ─────────────────────────────────────────────────────
    enum Shortcuts {
        static let sheet = "shortcuts.sheet"
        static let done = "shortcuts.done"
    }

    /// ── Settings window ─────────────────────────────────────────────────────
    enum Settings {
        static let appearanceTab = "settings.tab.appearance"
        static let typographyTab = "settings.tab.typography"
        static let libraryTab = "settings.tab.library"
        static let extensionsTab = "settings.tab.extensions"
        static let themePicker = "settings.theme"
        static let fontPicker = "settings.font"
        static let sizePicker = "settings.size"
        static let widthPicker = "settings.width"
        static let lineHeightPicker = "settings.lineHeight"
        static let typographyPreview = "settings.typographyPreview"
        static let changeLibrary = "settings.changeLibrary"
    }
}
