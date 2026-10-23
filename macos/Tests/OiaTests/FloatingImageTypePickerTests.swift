// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI
import XCTest

@MainActor
final class FloatingImageTypePickerTests: XCTestCase {
    func testOneNativeSelectorUsesSharedGlassAndKeepsItsHeight() throws {
        let host = hostingView(selection: Selection(.all), count: 12)
        let control = try XCTUnwrap(descendants(of: host, type: NSSegmentedControl.self).first)
        XCTAssertEqual(descendants(of: host, type: NSSegmentedControl.self).count, 1)
        XCTAssertEqual(control.trackingMode, .selectOne)
        XCTAssertEqual(host.fittingSize.height, 36, accuracy: 0.5)
        XCTAssertGreaterThan(host.fittingSize.width, 0)
        XCTAssertNil(control.selectedSegmentBezelColor)
        if #available(macOS 26.0, *) {
            let glass = try XCTUnwrap(descendants(of: host, type: NSGlassEffectView.self).first)
            XCTAssertTrue(control.isDescendant(of: try XCTUnwrap(glass.contentView)))
            XCTAssertFalse(try XCTUnwrap(control.cell as? NSSegmentedCell).isBordered)
            XCTAssertNil(glass.tintColor)
        }
    }

    func testNativeActionsSelectExactlyOneFacetIncludingRepeatedSelection() throws {
        let selection = Selection(.all)
        let host = hostingView(selection: selection, count: 12)
        let control = try XCTUnwrap(descendants(of: host, type: NSSegmentedControl.self).first)
        let width = host.fittingSize.width
        for (index, filter) in ImageTypeFilter.allCases.enumerated() {
            for _ in 0..<2 {
                control.selectedSegment = index
                XCTAssertTrue(control.sendAction(try XCTUnwrap(control.action), to: control.target))
                XCTAssertEqual(selection.value, filter)
                XCTAssertEqual((0..<control.segmentCount).filter { control.isSelected(forSegment: $0) }, [index])
                XCTAssertEqual(host.fittingSize.height, 36, accuracy: 0.5)
                XCTAssertEqual(host.fittingSize.width, width, accuracy: 0.5)
            }
        }
    }

    func testCountAndKeyboardSelectionUpdatesReuseTheNativeControl() throws {
        let selection = Selection(.graphics)
        let host = hostingView(selection: selection, count: 12)
        let original = try XCTUnwrap(descendants(of: host, type: NSSegmentedControl.self).first)
        let width = host.fittingSize.width
        for count in [0, 12, 123] {
            selection.value = .photos
            host.rootView = picker(selection: selection, count: count)
            host.layoutSubtreeIfNeeded()
            let control = try XCTUnwrap(descendants(of: host, type: NSSegmentedControl.self).first)
            XCTAssertTrue(control === original)
            XCTAssertEqual(control.selectedSegment, 2)
            XCTAssertEqual(labels(control), count > 0 ? ["All", "Graphics", "Photos", "Unsure (\(count))"] : ["All", "Graphics", "Photos"])
            XCTAssertEqual(host.fittingSize.height, 36, accuracy: 0.5)
            if count == 0 { XCTAssertLessThan(host.fittingSize.width, width) }
        }
    }

    func testZeroAndNegativeCountsHideUnsureAndDisplayAllForAnUnavailableSelection() throws {
        for count in [0, -1] {
            let host = hostingView(selection: Selection(.unsure), count: count)
            let control = try XCTUnwrap(descendants(of: host, type: NSSegmentedControl.self).first)
            XCTAssertEqual(labels(control), ["All", "Graphics", "Photos"])
            XCTAssertEqual(control.selectedSegment, 0)
        }
    }

    func testFilterShortcutHintsStayOnNativeSegments() throws {
        let host = hostingView(selection: Selection(.photos), count: 12)
        let control = try XCTUnwrap(descendants(of: host, type: NSSegmentedControl.self).first)
        XCTAssertEqual((0..<control.segmentCount).compactMap { control.toolTip(forSegment: $0) }, ["All (A)", "Graphics (G)", "Photos (P)", "Unsure (12) (U)"])
    }

    private func labels(_ control: NSSegmentedControl) -> [String] {
        (0..<control.segmentCount).compactMap { control.label(forSegment: $0) }
    }

    private func descendants<T: NSView>(of view: NSView, type: T.Type) -> [T] {
        (view as? T).map { [$0] } ?? view.subviews.flatMap { descendants(of: $0, type: type) }
    }

    private final class Selection {
        var value: ImageTypeFilter
        init(_ value: ImageTypeFilter) { self.value = value }
    }

    private func picker(selection: Selection, count: Int) -> FloatingImageTypePicker {
        FloatingImageTypePicker(
            selection: Binding(get: { selection.value }, set: { selection.value = $0 }),
            unsureImageCount: count
        )
    }

    private func hostingView(selection: Selection, count: Int) -> NSHostingView<FloatingImageTypePicker> {
        let host = NSHostingView(rootView: picker(selection: selection, count: count))
        host.frame = CGRect(x: 0, y: 0, width: 800, height: 36)
        host.layoutSubtreeIfNeeded()
        return host
    }
}
