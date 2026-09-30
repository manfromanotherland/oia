// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// The reading overlay: article content plus inspector actions.
struct ReaderPage {
    let app: XCUIApplication

    // ── Header ────────────────────────────────────────────────────────────

    var title: XCUIElement {
        app.byId(A11y.Detail.title)
    }

    var tags: XCUIElement {
        app.byId(A11y.Detail.tags)
    }

    var emptyState: XCUIElement {
        app.byId(A11y.Detail.empty)
    }

    /// The header's tag-chips text ("#a #b …"), or "" when the article has no tags
    /// (the label is hidden). SwiftUI carries this Text's string in the element's
    /// `.value` on some renders and `.label` on others (the same quirk the list
    /// rows hit), and the identifier can resolve to an empty wrapper, so scan every
    /// static text carrying the identifier and return the first non-empty of the
    /// two, then fall back to the any-type match.
    var tagsText: String {
        for element in app.staticTexts.matching(identifier: A11y.Detail.tags).allElementsBoundByIndex {
            if !element.label.isEmpty {
                return element.label
            }
            if let value = element.value as? String, !value.isEmpty {
                return value
            }
        }
        let any = tags
        if any.exists {
            if !any.label.isEmpty {
                return any.label
            }
            if let value = any.value as? String, !value.isEmpty {
                return value
            }
        }
        return ""
    }

    /// Polls until the header shows a chip for `tag` (tag edits reconcile
    /// asynchronously after the write).
    @discardableResult
    func waitForTag(_ tag: String, timeout: TimeInterval = 8) -> Bool {
        poll(timeout: timeout) { tagsText.localizedCaseInsensitiveContains("#\(tag)") }
    }

    /// Polls until the header no longer shows a chip for `tag`.
    @discardableResult
    func waitForNoTag(_ tag: String, timeout: TimeInterval = 8) -> Bool {
        poll(timeout: timeout) { !tagsText.localizedCaseInsensitiveContains("#\(tag)") }
    }

    private func poll(timeout: TimeInterval, until condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if condition() {
                return true
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        return false
    }

    /// The reader's title text. The `.accessibilityIdentifier` can resolve to a
    /// wrapper whose own label is empty, so prefer the `staticText` carrying the
    /// identifier; poll briefly since the detail loads asynchronously.
    var titleText: String {
        let deadline = Date().addingTimeInterval(4)
        repeat {
            let text = app.staticTexts.matching(identifier: A11y.Detail.title).firstMatch
            if text.exists, !text.label.isEmpty {
                return text.label
            }
            // Every `any` read stays behind `exists`: while the reader loads a
            // freshly-opened article it shows a spinner with no title element, and
            // reading `.value` off an absent element throws a snapshot error rather
            // than returning empty — which a tight polling caller (e.g. a
            // "reader followed the selection" wait) would surface as a hard failure
            // instead of simply polling again.
            let any = title
            if any.exists {
                if !any.label.isEmpty {
                    return any.label
                }
                if let value = any.value as? String, !value.isEmpty {
                    return value
                }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        return ""
    }

    // ── Body ──────────────────────────────────────────────────────────────
    // The Markdown body renders into AppKit text views without identifiers, so
    // body assertions match on rendered text anywhere in the reader.

    func bodyContains(_ text: String, timeout: TimeInterval = 8) -> Bool {
        // Scope to text-bearing element types — the reader body renders into
        // AppKit static texts / text views — rather than `.descendants(.any)`,
        // which is far too slow over a full article and times out.
        let predicate = NSPredicate(format: "label CONTAINS[c] %@ OR value CONTAINS[c] %@", text, text)
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if app.staticTexts.matching(predicate).firstMatch.exists {
                return true
            }
            if app.textViews.matching(predicate).firstMatch.exists {
                return true
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        } while Date() < deadline
        return false
    }

    /// Whether any image is present in the reader (the kitchen-sink asset).
    var hasImage: Bool {
        app.images.firstMatch.exists
    }

    /// Width of the reader's body column, in points. The body's selectable runs
    /// render into AppKit text views that fill the measure, so the widest one
    /// tracks the Width typography setting (see `ReaderWidth`). Only text views
    /// are measured — the board renders SwiftUI `Text` (static texts), so it
    /// can't pollute the reading. Returns 0 when the body hasn't
    /// rendered yet.
    var bodyWidth: CGFloat {
        app.textViews.allElementsBoundByIndex
            .filter(\.exists)
            .map(\.frame.width)
            .max() ?? 0
    }

    /// Polls until the body column settles within `tolerance` of `expected` —
    /// a width change reflows asynchronously.
    @discardableResult
    func waitForBodyWidth(_ expected: CGFloat, tolerance: CGFloat = 40,
                          timeout: TimeInterval = 8) -> Bool
    {
        poll(timeout: timeout) { abs(bodyWidth - expected) <= tolerance }
    }

    // ── Image zoom lightbox ───────────────────────────────────────────────────

    /// The first tappable figure in the reader body.
    var figure: XCUIElement {
        app.byId(A11y.Reader.figure)
    }

    var lightboxImage: XCUIElement {
        app.byId(A11y.Lightbox.image)
    }

    var lightboxClose: XCUIElement {
        app.byId(A11y.Lightbox.close)
    }

    /// Scroll the reader until the figure renders (the body lazily builds blocks,
    /// so an image near the end isn't in the tree until scrolled toward).
    @discardableResult
    func revealFigure(timeout: TimeInterval = 10) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while !figure.exists, Date() < deadline {
            let scrollViews = app.scrollViews.allElementsBoundByIndex
            if scrollViews.isEmpty {
                app.swipeUp()
            } else {
                for scrollView in scrollViews where scrollView.exists {
                    scrollView.swipeUp()
                    if figure.exists {
                        break
                    }
                }
            }
        }
        return figure.exists
    }

    /// Click the figure to open the zoom lightbox.
    func openImageZoom() {
        figure.clickWhenReady()
    }

    // ── Oversize guard ──────────────────────────────────────────────────────

    var oversizeNotice: XCUIElement {
        app.byId(A11y.Detail.oversize)
    }

    var oversizeOpenInBrowserButton: XCUIElement {
        app.byId(A11y.Detail.oversizeOpenInBrowser)
    }

    // ── Toolbar actions ─────────────────────────────────────────────────────

    var closeButton: XCUIElement {
        app.byId(A11y.Detail.close)
    }

    func close() {
        closeButton.clickWhenReady()
    }

    func openInBrowser() {
        app.byId(A11y.Toolbar.openInBrowser).clickWhenReady()
    }

    func openTagPicker() {
        let edit = app.byId(A11y.Inspector.editTags)
        if !edit.exists {
            app.byId(A11y.Inspector.toggle).clickWhenReady()
        }
        edit.clickWhenReady()
    }

    func showInspector() {
        let button = app.buttons["Show Inspector"]
        if button.exists {
            button.clickWhenReady()
        }
    }

    /// Highlight the reader's current text selection. With nothing selected this
    /// raises `highlightHint` instead (the inspector is opened with ⌘⇧H — see
    /// `Keyboard.toggleHighlights`).
    func highlightSelection() {
        app.byId(A11y.Toolbar.highlight).clickWhenReady()
    }

    var highlightHint: XCUIElement {
        app.byId(A11y.Toolbar.highlightHint)
    }

    func delete() {
        app.byId(A11y.Toolbar.delete).clickWhenReady()
    }
}
