// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct DetailNavigationActions {
    let canMovePrevious: Bool
    let canMoveNext: Bool
    let showsInspector: Bool
    let movePrevious: () -> Void
    let moveNext: () -> Void
    let toggleInspector: () -> Void
}

/// View-local actions that menu commands can invoke without moving board
/// presentation or search-focus state into the shared app model.
struct BoardActions {
    let canOpenSelection: Bool
    let canQuickLookSelection: Bool
    let canFocusSearch: Bool
    let canScrollToTop: Bool
    let openSelection: () -> Void
    let editTags: (String) -> Void
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

extension FocusedValues {
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
            .disabled(detailNavigationActions != nil || appState.isEditingText)

            Divider()

            Button("Decrease Card Size") {
                if let smaller = cardSize.smaller {
                    cardSize = smaller
                }
            }
            .keyboardShortcut(ShortcutCatalog.decreaseCardSize)
            .disabled(detailNavigationActions != nil || cardSize.smaller == nil)

            Button("Increase Card Size") {
                if let larger = cardSize.larger {
                    cardSize = larger
                }
            }
            .keyboardShortcut(ShortcutCatalog.increaseCardSize)
            .disabled(detailNavigationActions != nil || cardSize.larger == nil)

            if let detailNavigationActions {
                Divider()

                Button("Previous Item", action: detailNavigationActions.movePrevious)
                    .keyboardShortcut(ShortcutCatalog.previousItem)
                    .disabled(!detailNavigationActions.canMovePrevious || appState.isEditingText)

                Button("Next Item", action: detailNavigationActions.moveNext)
                    .keyboardShortcut(ShortcutCatalog.nextItem)
                    .disabled(!detailNavigationActions.canMoveNext || appState.isEditingText)

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
}
