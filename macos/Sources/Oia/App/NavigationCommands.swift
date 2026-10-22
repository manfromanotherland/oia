// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct DetailNavigationActions {
    let canMovePrevious: Bool
    let canMoveNext: Bool
    let showsInspector: Bool
    let canMarkAsPhoto: Bool
    let movePrevious: () -> Void
    let moveNext: () -> Void
    let toggleInspector: () -> Void
    let markAsPhoto: () -> Void
}

struct DetailViewActions {
    let firstTitle: String
    let originalTitle: String
    let canSelectFirst: Bool
    let canSelectOriginal: Bool
    let canAdjustTypography: Bool
    let selectFirst: () -> Void
    let selectOriginal: () -> Void
}

/// View-local actions that menu commands can invoke without moving board
/// presentation or search-focus state into the shared app model.
struct BoardActions {
    let canOpenSelection: Bool
    let canQuickLookSelection: Bool
    let canFocusSearch: Bool
    let canScrollToTop: Bool
    let canMarkAsPhoto: Bool
    let openSelection: () -> Void
    let editTags: () -> Void
    let markAsPhoto: () -> Void
    let toggleQuickLook: () -> Void
    let focusSearch: () -> Void
    let scrollToTop: () -> Void
}

private struct DetailNavigationActionsKey: FocusedValueKey {
    typealias Value = DetailNavigationActions
}

private struct BoardActionsKey: FocusedValueKey {
    typealias Value = BoardActions
}

private struct DetailViewActionsKey: FocusedValueKey {
    typealias Value = DetailViewActions
}

extension FocusedValues {
    var detailViewActions: DetailViewActions? {
        get { self[DetailViewActionsKey.self] }
        set { self[DetailViewActionsKey.self] = newValue }
    }

    var detailNavigationActions: DetailNavigationActions? {
        get { self[DetailNavigationActionsKey.self] }
        set { self[DetailNavigationActionsKey.self] = newValue }
    }

    var boardActions: BoardActions? {
        get { self[BoardActionsKey.self] }
        set { self[BoardActionsKey.self] = newValue }
    }
}

/// Menu bar commands for navigating the main window.
struct NavigationCommands: Commands {
    var appState: AppState
    @AppStorage("cardSize", store: AppDefaults.store) private var cardSize: CardSize = .small
    @FocusedValue(\.detailNavigationActions) private var detailNavigationActions
    @FocusedValue(\.detailViewActions) private var detailViewActions
    @FocusedValue(\.boardActions) private var boardActions

    var body: some Commands {
        CommandGroup(replacing: .sidebar) {
            Button("Focus Search") {
                boardActions?.focusSearch()
            }
            .keyboardShortcut(ShortcutCatalog.focusSearch)
            .disabled(boardActions?.canFocusSearch != true)

            Button("Scroll to Top") {
                boardActions?.scrollToTop()
            }
            .keyboardShortcut(ShortcutCatalog.scrollToTop)
            .disabled(boardActions?.canScrollToTop != true || appState.isEditingText)

            Button(appState.isFocusMode ? "Exit Focus Mode" : "Focus Mode") {
                appState.isFocusMode.toggle()
            }
            .keyboardShortcut(ShortcutCatalog.toggleFocusMode)

            Divider()

            if detailNavigationActions == nil {
                filterMenu
            }

            if let detailViewActions {
                Button(detailViewActions.firstTitle, action: detailViewActions.selectFirst)
                    .keyboardShortcut(ShortcutCatalog.selectFirstView)
                    .disabled(!detailViewActions.canSelectFirst)

                Button(detailViewActions.originalTitle, action: detailViewActions.selectOriginal)
                    .keyboardShortcut(ShortcutCatalog.selectOriginalView)
                    .disabled(!detailViewActions.canSelectOriginal)
            }

            if detailNavigationActions == nil {
                Divider()

                Button("Zoom Out") {
                    if let smaller = cardSize.smaller {
                        cardSize = smaller
                    }
                }
                .keyboardShortcut(ShortcutCatalog.decreaseCardSize)
                .disabled(boardActions == nil || appState.isEditingText || cardSize.smaller == nil)

                Button("Zoom In") {
                    if let larger = cardSize.larger {
                        cardSize = larger
                    }
                }
                .keyboardShortcut(ShortcutCatalog.increaseCardSize)
                .disabled(boardActions == nil || appState.isEditingText || cardSize.larger == nil)
            }

            if let detailNavigationActions {
                Divider()

                Button("Previous Item", action: detailNavigationActions.movePrevious)
                    .keyboardShortcut(ShortcutCatalog.previousItem)
                    .disabled(
                        !detailNavigationActions.canMovePrevious
                            || appState.isEditingText || appState.isBrowsingWebContent
                    )

                Button("Next Item", action: detailNavigationActions.moveNext)
                    .keyboardShortcut(ShortcutCatalog.nextItem)
                    .disabled(
                        !detailNavigationActions.canMoveNext
                            || appState.isEditingText || appState.isBrowsingWebContent
                    )

                Divider()

                Button(
                    detailNavigationActions.showsInspector ? "Hide Sidebar" : "Show Sidebar",
                    action: detailNavigationActions.toggleInspector
                )
                .keyboardShortcut(ShortcutCatalog.toggleSidebar)
                .disabled(appState.isEditingText)
            }
        }
    }

    private var filterMenu: some View {
        Menu("Filter") {
            ForEach(LibraryScope.allCases) { scope in
                Button(scope.label) {
                    appState.selectScope(scope)
                }
                .keyboardShortcut(ShortcutCatalog.filterShortcut(for: scope))
                .disabled(appState.activeScope == scope)
            }

            Divider()

            Button("Previous Filter") {
                appState.selectScope(appState.activeScope.previous)
            }
            .keyboardShortcut(ShortcutCatalog.previousFilter)

            Button("Next Filter") {
                appState.selectScope(appState.activeScope.next)
            }
            .keyboardShortcut(ShortcutCatalog.nextFilter)
        }
        .disabled(appState.isEditingText)
    }
}
