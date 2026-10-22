// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import XCTest

@MainActor
final class BoardScopeControlTests: XCTestCase {
    func testExpandedImagesKeepsTheNativeSegmentedControlHeight() {
        for size in [ControlSize.mini, .small, .regular, .large] {
            let native = fittingSize(nativePicker.controlSize(size))
            let collapsed = fittingSize(control(scope: .all).controlSize(size))
            let expanded = fittingSize(control(scope: .images).controlSize(size))

            XCTAssertGreaterThan(native.height, 0)
            XCTAssertEqual(collapsed.height, native.height, accuracy: 0.5)
            XCTAssertEqual(expanded.height, native.height, accuracy: 0.5)
            XCTAssertGreaterThan(expanded.width, collapsed.width)
        }
    }

    func testUnsureAndEveryImageSelectionKeepTheSameHeight() {
        let nativeHeight = fittingSize(nativePicker).height
        for selection in ImageTypeFilter.allCases {
            for scheme in [ColorScheme.light, .dark] {
                let measured = fittingSize(
                    control(scope: .images, imageType: selection, hasUnsureImages: true)
                        .environment(\.colorScheme, scheme)
                )
                XCTAssertEqual(measured.height, nativeHeight, accuracy: 0.5)
            }
        }
        XCTAssertGreaterThan(
            fittingSize(control(scope: .images, hasUnsureImages: true)).width,
            fittingSize(control(scope: .images)).width
        )
    }

    func testOtherScopesCollapseTheRememberedImageFacets() {
        let collapsedSize = fittingSize(control(scope: .all))
        for scope in LibraryScope.allCases where scope != .images {
            let measured = fittingSize(
                control(scope: scope, imageType: .photos, hasUnsureImages: true)
            )
            XCTAssertEqual(measured.width, collapsedSize.width, accuracy: 0.5)
            XCTAssertEqual(measured.height, collapsedSize.height, accuracy: 0.5)
        }
    }

    private var nativePicker: some View {
        Picker("Filter", selection: .constant(LibraryScope.all)) {
            ForEach(LibraryScope.allCases) { scope in
                Text(scope.label).tag(scope)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
    }

    private func control(
        scope: LibraryScope, imageType: ImageTypeFilter = .all, hasUnsureImages: Bool = false
    ) -> some View {
        BoardScopeControl(
            scope: .constant(scope), imageType: .constant(imageType),
            imageTypes: ImageTypeFilter.availableCases(hasUnsureImages: hasUnsureImages)
        )
    }

    private func fittingSize(_ view: some View) -> CGSize {
        NSHostingView(rootView: view).fittingSize
    }
}
