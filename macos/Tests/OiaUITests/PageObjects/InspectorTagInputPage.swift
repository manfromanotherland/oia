// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// The inline Tag input in a reading's Inspector.
struct InspectorTagInputPage {
    let app: XCUIApplication

    var field: XCUIElement {
        app.byId(A11y.Inspector.tagInput)
    }

    var isVisible: Bool {
        field.exists
    }

    func add(_ tag: String) {
        field.clickWhenReady()
        field.typeText(tag)
        field.typeKey(.return, modifierFlags: [])
    }

    func remove(_ tag: String) {
        app.byId(A11y.Inspector.removeTag(tag)).clickWhenReady()
        app.buttons["Remove tag"].clickWhenReady()
    }
}
