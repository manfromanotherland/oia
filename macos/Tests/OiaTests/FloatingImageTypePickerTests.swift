// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI
import XCTest

@MainActor
final class FloatingImageTypePickerTests: XCTestCase {
    func testNativeGlassButtonsKeepTheSameSizeAcrossSelections() throws {
        guard #available(macOS 26.0, *) else {
            throw XCTSkip("Native glass buttons require macOS 26")
        }
        var widths: [CGFloat] = []
        for filter in ImageTypeFilter.allCases {
            let host = hostingView(selection: Selection(filter), count: 12)
            XCTAssertGreaterThan(host.fittingSize.width, 0)
            XCTAssertEqual(host.fittingSize.height, 36, accuracy: 0.5)
            widths.append(host.fittingSize.width)
        }
        let firstWidth = try XCTUnwrap(widths.first)
        for width in widths {
            XCTAssertEqual(width, firstWidth, accuracy: 0.5)
        }
    }

    func testZeroAndNegativeCountsHideUnsureWithoutChangingTheSelectedFacet() {
        for count in [0, -1] {
            let view = picker(selection: Selection(.photos), count: count)
            XCTAssertEqual(view.availableFilters, [.all, .graphics, .photos])
            XCTAssertTrue(view.selectionBinding(for: .photos).wrappedValue)
            XCTAssertFalse(view.selectionBinding(for: .unsure).wrappedValue)
        }
    }

    func testCountUpdatesShowAndHideUnsureWhilePreservingSelectedFacet() {
        let selection = Selection(.graphics)
        let host = hostingView(selection: selection, count: 12)
        let expandedWidth = host.fittingSize.width
        for count in [0, 12] {
            let view = picker(selection: selection, count: count)
            host.rootView = view
            host.layoutSubtreeIfNeeded()
            XCTAssertEqual(view.availableFilters, count > 0 ? [.all, .graphics, .photos, .unsure] : [.all, .graphics, .photos])
            XCTAssertTrue(view.selectionBinding(for: .graphics).wrappedValue)
            if count == 0 {
                XCTAssertLessThan(host.fittingSize.width, expandedWidth)
            } else {
                XCTAssertEqual(host.fittingSize.width, expandedWidth, accuracy: 0.5)
            }
        }
    }

    func testNativeToggleSelectionIsExclusiveAndCannotBeTurnedOff() {
        let selection = Selection(.all)
        let view = picker(selection: selection, count: 12)
        for filter in ImageTypeFilter.allCases {
            let binding = view.selectionBinding(for: filter)
            binding.wrappedValue = true
            XCTAssertEqual(selection.value, filter)
            binding.wrappedValue = false
            XCTAssertEqual(selection.value, filter)
            XCTAssertEqual(view.availableFilters.filter { view.selectionBinding(for: $0).wrappedValue }, [filter])
        }
    }

    func testHiddenUnsureSelectionDisplaysAllImages() {
        let view = picker(selection: Selection(.unsure), count: 0)
        XCTAssertEqual(view.availableFilters, [.all, .graphics, .photos])
        XCTAssertTrue(view.selectionBinding(for: .all).wrappedValue)
        XCTAssertFalse(view.selectionBinding(for: .unsure).wrappedValue)
    }

    private final class Selection {
        var value: ImageTypeFilter

        init(_ value: ImageTypeFilter) {
            self.value = value
        }
    }

    private func picker(selection: Selection, count: Int) -> FloatingImageTypePicker {
        FloatingImageTypePicker(
            selection: Binding(get: { selection.value }, set: { selection.value = $0 }),
            unsureImageCount: count
        )
    }

    private func hostingView(selection: Selection, count: Int) -> NSHostingView<FloatingImageTypePicker> {
        let host = NSHostingView(rootView: picker(selection: selection, count: count))
        host.frame = CGRect(x: 0, y: 0, width: 800, height: 80)
        host.layoutSubtreeIfNeeded()
        return host
    }
}
