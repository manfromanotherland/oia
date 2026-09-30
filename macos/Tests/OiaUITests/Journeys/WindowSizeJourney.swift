// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

final class WindowSizeJourney: UITestCase {
    func testWindowSizeSurvivesRelaunch() throws {
        try launchApp { options in
            // Native SwiftUI window frames use the app's standard defaults,
            // while this test's preference store is isolated per run.
            guard let suite = options.defaultsSuite,
                  let store = UserDefaults(suiteName: suite)
            else { return }
            store.set([1100.0, 720.0], forKey: "mainWindowContentSize")
        }
        let window = app.windows.firstMatch
        let initialFrame = window.frame

        let resizeCorner = window.coordinate(
            withNormalizedOffset: CGVector(dx: 0.997, dy: 0.997)
        )
        resizeCorner.press(
            forDuration: 0.2,
            thenDragTo: resizeCorner.withOffset(CGVector(dx: -120, dy: -80))
        )

        let resizedFrame = window.frame
        XCTAssertLessThan(resizedFrame.width, initialFrame.width - 40)
        XCTAssertLessThan(resizedFrame.height, initialFrame.height - 40)

        relaunchApp()
        let restoredFrame = app.windows.firstMatch.frame
        XCTAssertEqual(restoredFrame.width, resizedFrame.width, accuracy: 15)
        XCTAssertEqual(restoredFrame.height, resizedFrame.height, accuracy: 15)
    }
}
