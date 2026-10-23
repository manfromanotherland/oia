// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI
import XCTest

@MainActor
final class FloatingImageTypePickerTests: XCTestCase {
    func testFloatingFilterPreservesTheSystemPickersSelectionAppearanceAndMetrics() throws {
        let window = NSWindow(
            contentRect: NSRect(x: -5000, y: -5000, width: 800, height: 100),
            styleMask: [.titled, .fullSizeContentView], backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        let controller = NSHostingController(rootView: PickerComparison())
        window.contentViewController = controller
        window.layoutIfNeeded()
        drainUpdates()
        let control = try nativeControl(in: controller.view)
        let root = try XCTUnwrap(window.contentView?.superview)
        let nativeReference = try XCTUnwrap(descendants(of: root, type: NSSegmentedControl.self).first {
            $0 !== control
        })
        defer { window.close() }
        XCTAssertFalse(window.isVisible)
        XCTAssertFalse(window.isKeyWindow)
        XCTAssertFalse(window.isMainWindow)
        let cell = try XCTUnwrap(control.cell)
        let referenceCell = try XCTUnwrap(nativeReference.cell)

        // The rejected implementation disabled the native bezel and drew its own
        // selection. Require the same untouched configuration as a system Picker.
        XCTAssertEqual(cell.isBordered, referenceCell.isBordered)
        XCTAssertEqual(cell.isBezeled, referenceCell.isBezeled)
        XCTAssertEqual(control.segmentStyle, nativeReference.segmentStyle)
        XCTAssertEqual(control.segmentDistribution, nativeReference.segmentDistribution)
        XCTAssertEqual(control.controlSize, nativeReference.controlSize)
        XCTAssertEqual(control.trackingMode, .selectOne)
        XCTAssertEqual(control.intrinsicContentSize, nativeReference.intrinsicContentSize)
        XCTAssertEqual(control.bounds.height, 36, accuracy: 0.5)
        XCTAssertNil(control.selectedSegmentBezelColor)
        XCTAssertEqual(descendants(of: controller.view, type: NSSegmentedControl.self).count, 1)
    }

    func testNativeActionsSelectExactlyOneFacetIncludingRepeatedSelection() throws {
        let selection = Selection(.all)
        let host = hostingView(selection: selection, count: 12)
        let control = try nativeControl(in: host)
        let width = host.fittingSize.width
        for (index, filter) in ImageTypeFilter.allCases.enumerated() {
            for _ in 0..<2 {
                control.selectedSegment = index
                XCTAssertTrue(control.sendAction(try XCTUnwrap(control.action), to: control.target))
                drainUpdates()
                XCTAssertEqual(selection.value, filter)
                XCTAssertEqual((0..<control.segmentCount).filter { control.isSelected(forSegment: $0) }, [index])
                XCTAssertEqual(host.fittingSize.height, 36, accuracy: 0.5)
                XCTAssertEqual(host.fittingSize.width, width, accuracy: 0.5)
            }
        }
    }

    func testCountAndMenuSelectionUpdatesReuseTheSystemControl() throws {
        let selection = Selection(.graphics)
        let host = hostingView(selection: selection, count: 12)
        let original = try nativeControl(in: host)
        let width = host.fittingSize.width
        for count in [0, 12, 123] {
            selection.value = .photos
            host.rootView = picker(selection: selection, count: count)
            host.layoutSubtreeIfNeeded()
            let control = try nativeControl(in: host)
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
            let control = try nativeControl(in: host)
            XCTAssertEqual(labels(control), ["All", "Graphics", "Photos"])
            XCTAssertEqual(control.selectedSegment, 0)
        }
    }

    private struct PickerComparison: View {
        var body: some View {
            FloatingImageTypePicker(selection: .constant(.graphics), unsureImageCount: 12)
                .frame(width: 800, height: 100)
                .toolbar {
                    ToolbarItem(placement: .principal) { ReferencePicker() }
                }
        }
    }

    private struct ReferencePicker: View {
        var body: some View {
            Picker("Image type", selection: .constant(ImageTypeFilter.graphics)) {
                Text("All").tag(ImageTypeFilter.all)
                Text("Graphics").tag(ImageTypeFilter.graphics)
                Text("Photos").tag(ImageTypeFilter.photos)
                Text("Unsure (12)").tag(ImageTypeFilter.unsure)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        }
    }

    private func labels(_ control: NSSegmentedControl) -> [String] {
        (0..<control.segmentCount).compactMap { control.label(forSegment: $0) }
    }

    private func nativeControl(in view: NSView) throws -> NSSegmentedControl {
        try XCTUnwrap(descendants(of: view, type: NSSegmentedControl.self).first)
    }

    private func descendants<T: NSView>(of view: NSView, type: T.Type) -> [T] {
        (view as? T).map { [$0] } ?? view.subviews.flatMap { descendants(of: $0, type: type) }
    }

    private func drainUpdates() {
        RunLoop.main.run(until: Date().addingTimeInterval(0.005))
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
        host.frame = CGRect(x: 0, y: 0, width: 800, height: 80)
        host.layoutSubtreeIfNeeded()
        return host
    }
}
