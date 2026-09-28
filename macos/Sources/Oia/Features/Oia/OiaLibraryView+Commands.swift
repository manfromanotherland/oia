// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

extension OiaLibraryView {
    var scopeSelection: Binding<LibraryScope> {
        Binding(
            get: { appState.activeScope },
            set: { appState.selectScope($0) }
        )
    }

    var focusedBoardActions: BoardActions {
        BoardActions(
            canOpenSelection: presentedReading == nil && singleSelectedRow != nil,
            canQuickLookSelection: presentedReading == nil
                && singleSelectedRow != nil
                && appState.libraryURL != nil,
            canFocusSearch: presentedReading == nil && !appState.isFocusMode,
            canScrollToTop: presentedReading == nil && !appState.readings.isEmpty,
            openSelection: openSelection,
            toggleQuickLook: toggleQuickLook,
            focusSearch: focusSearch,
            scrollToTop: scrollBoardToTop
        )
    }

    var boardMagnifyGesture: some Gesture {
        MagnifyGesture(minimumScaleDelta: 0.02)
            .onChanged { value in
                let startingSize = pinchStartCardSize ?? cardSize
                pinchStartCardSize = startingSize
                cardSize = startingSize.zoomed(by: value.magnification)
            }
            .onEnded { _ in
                pinchStartCardSize = nil
            }
    }

    var singleSelectedRow: ReadingRow? {
        let rows = appState.selectedRows
        return rows.count == 1 ? rows.first : nil
    }

    func openSelection() {
        guard presentedReading == nil, let row = singleSelectedRow else { return }
        open(row)
    }

    func scrollBoardToTop() {
        guard presentedReading == nil, !appState.readings.isEmpty else { return }
        boardPosition.scrollToStart(animated: !accessibilityReduceMotion)
    }

    static let boardSpacing: CGFloat = 18
    static let boardTopSpacing: CGFloat = 12
}
