// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI
import XCTest

@MainActor
final class FloatingImageTypePickerTests: XCTestCase {
    func testZeroCountKeepsUnsureVisibleAndDisablesItsNativeSegment() throws {
        let host = hostingView(selection: .photos, count: 0)
        let control = try XCTUnwrap(segmentedControl(in: host))

        XCTAssertEqual(control.segmentCount, 4)
        XCTAssertEqual((0 ..< control.segmentCount).map { control.label(forSegment: $0) },
                       ["All", "Graphics", "Photos", "Unsure (0)"])
        XCTAssertEqual(control.selectedSegment, 2)
        XCTAssertFalse(control.isEnabled(forSegment: 3))
        for index in 0 ..< 3 {
            XCTAssertTrue(control.isEnabled(forSegment: index))
        }
    }

    func testCountUpdatesEnableAndDisableUnsureWhilePreservingSelectedFacet() throws {
        let host = hostingView(selection: .graphics, count: 12)
        let initial = try XCTUnwrap(segmentedControl(in: host))
        XCTAssertEqual(initial.label(forSegment: 3), "Unsure (12)")
        XCTAssertTrue(initial.isEnabled(forSegment: 3))
        XCTAssertEqual(initial.selectedSegment, 1)

        for count in [0, 12] {
            host.rootView = FloatingImageTypePicker(selection: .constant(.graphics), unsureImageCount: count)
            host.layoutSubtreeIfNeeded()
            let control = try XCTUnwrap(segmentedControl(in: host))
            XCTAssertEqual(control.label(forSegment: 3), "Unsure (\(count))")
            XCTAssertEqual(control.isEnabled(forSegment: 3), count > 0)
            XCTAssertEqual(control.selectedSegment, 1)
        }
    }

    private func hostingView(selection: ImageTypeFilter, count: Int) -> NSHostingView<FloatingImageTypePicker> {
        let host = NSHostingView(rootView:
            FloatingImageTypePicker(selection: .constant(selection), unsureImageCount: count)
        )
        host.frame = CGRect(x: 0, y: 0, width: 800, height: 80)
        host.layoutSubtreeIfNeeded()
        return host
    }

    private func segmentedControl(in view: NSView) -> NSSegmentedControl? {
        if let control = view as? NSSegmentedControl { return control }
        return view.subviews.lazy.compactMap { self.segmentedControl(in: $0) }.first
    }
}
