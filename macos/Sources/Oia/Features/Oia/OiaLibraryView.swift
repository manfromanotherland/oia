// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import LazyLayoutKit
import QuickLook
import SwiftUI
import UniformTypeIdentifiers

struct OiaLibraryView: View {
    @Environment(AppState.self) var appState
    @Environment(\.accessibilityReduceMotion) private var accessibilityReduceMotion
    @Environment(\.displayScale) private var displayScale
    @Environment(\.scenePhase) private var scenePhase
    @AppStorage("cardSize", store: AppDefaults.store) var cardSize: CardSize = .small

    @State var presentedReading: ReadingRow?
    @State private var gallerySnapshot = GallerySnapshot<ReadingRow>()
    @State private var tagTargetID: String?
    @State private var isDropTargeted = false
    @State var cardTextMetrics = OiaCardTextMetrics()
    @State private var videoPlaybackPositions = VideoPlaybackPositionStore()
    @State var boardPosition = LazyLayoutPosition<String>()
    @State var boardNavigation = MasonryNavigationCoordinator<ReadingRow, String>()
    @State var boardModifierKeys: EventModifiers = []
    @State private var boardScrollState = BoardScrollState()
    @State var pinchStartCardSize: CardSize?
    @State var quickLookURL: URL?
    @FocusState var boardFocused: Bool
    @FocusState var searchFocused: Bool
    var body: some View {
        NavigationStack {
            deletionSurface
                .navigationDestination(isPresented: detailPresented) {
                    overlay
                }
        }
        .sheet(isPresented: tagSheetPresented) {
            tagPicker
        }
        .environment(\.assetContentGeneration, appState.libraryContentGeneration)
        .focusedSceneValue(\.boardActions, focusedBoardActions)
        .quickLookPreview($quickLookURL)
        .alert("Óia couldn’t complete that action", isPresented: errorAlertPresented) {
            Button("OK") { appState.error = nil }
        } message: {
            Text(appState.error ?? "An unknown error occurred.")
        }
    }
}

private struct SearchQueryChangeModifier: ViewModifier {
    @Environment(AppState.self) private var appState

    func body(content: Content) -> some View {
        content.onChange(of: appState.activeSearchInput) { _, _ in
            appState.searchDidChange()
        }
    }
}

extension OiaLibraryView {
    private var layeredSurface: some View {
        ZStack {
            librarySurface

            if isDropTargeted {
                dropPrompt
            }

            if let notice = appState.saveNotice {
                saveNotice(notice)
            }
        }
    }

    private var ingestibleSurface: some View {
        layeredSurface
            .animation(.easeInOut(duration: 0.2), value: presentedReading?.id)
            .onDrop(of: supportedDropTypes, isTargeted: $isDropTargeted) { providers in
                guard !providers.isEmpty else { return false }
                save(providers)
                return true
            }
            .onPasteCommand(
                of: supportedPasteTypes,
                validator: { providers in
                    appState.isEditingText || providers.isEmpty ? nil : providers
                },
                perform: save
            )
            .background {
                if TestHooks.isUITesting {
                    rowsProbe
                }
            }
    }

    private var reactiveSurface: some View {
        ingestibleSurface
            .modifier(SearchQueryChangeModifier())
            .onChange(of: appState.readings) { _, rows in
                guard let id = presentedReading?.id else { return }
                gallerySnapshot.reconcile(rows)
                if let refreshed = gallerySnapshot.row(id: id) {
                    updatePresentedRow(refreshed)
                } else {
                    advanceOverlayPastCurrent()
                }
            }
    }

    private var deletionSurface: some View {
        reactiveSurface
            .confirmationDialog(
                deleteDialogTitle,
                isPresented: deleteDialogPresented,
                presenting: appState.pendingDelete
            ) { rows in
                Button(deleteButtonTitle(for: rows), role: .destructive) {
                    delete(rows)
                }
                Button("Cancel", role: .cancel) {}
            } message: { rows in
                deleteDialogMessage(for: rows)
            }
    }
}

extension OiaLibraryView {
    private var librarySurface: some View {
        detailSurface
    }

    @ViewBuilder
    private var detailSurface: some View {
        if appState.isFocusMode {
            board
        } else {
            searchableBoard
        }
    }

    private var searchableBoard: some View {
        @Bindable var bindableAppState = appState
        let isSearchExpanded = searchFocused
            || !bindableAppState.searchQuery.isEmpty
            || !bindableAppState.searchTokens.isEmpty
        return board
            .searchable(
                text: $bindableAppState.searchQuery,
                tokens: Binding(
                    get: { appState.searchTokens },
                    set: { appState.searchTokens = BoardSearchCriteria(tokens: $0).tokens }
                ),
                placement: .toolbar,
                prompt: "Search Óia"
            ) { token in
                Text(token.displayValue)
            }
            .searchSuggestions {
                nativeSearchSuggestions
            }
            .searchFocused($searchFocused)
            .onSubmit(of: .search) {
                let completion = BoardSearchTermCompletion(
                    text: appState.searchQuery,
                    tokens: appState.searchTokens
                )
                guard completion.didComplete else { return }
                appState.searchTokens = completion.tokens
                appState.searchQuery = completion.text
            }
            .toolbar { boardToolbar }
            .background {
                CompactSearchToolbarConfiguration(
                    isSearchExpanded: isSearchExpanded,
                    searchTokens: appState.searchTokens
                )
                    .frame(width: 0, height: 0)
            }
    }

    @ToolbarContentBuilder
    private var boardToolbar: some ToolbarContent {
        if presentedReading == nil {
            ToolbarItem(placement: .navigation) {
                cardSizeControl
            }

            ToolbarItem(placement: .principal) {
                boardFilterPicker
            }
        }
    }

    private var boardFilterPicker: some View {
        Picker("Filter", selection: scopeSelection) {
            ForEach(LibraryScope.allCases) { scope in
                Label(scope.label, systemImage: scope.icon)
                    .labelStyle(.iconOnly)
                    .help(scope.label)
                    .accessibilityLabel(scope.label)
                    .tag(scope)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .accessibilityLabel("Filter")
        .accessibilityValue(appState.activeScope.label)
        .accessibilityIdentifier(A11y.Filter.group)
    }

    private var cardSizeControl: some View {
        ControlGroup("Card Size") {
            Button {
                if let smaller = cardSize.smaller {
                    cardSize = smaller
                }
            } label: {
                Label("Decrease Card Size", systemImage: "minus")
            }
            .disabled(cardSize.smaller == nil)
            .help("Decrease card size (\(ShortcutCatalog.decreaseCardSize.display))")

            Button {
                if let larger = cardSize.larger {
                    cardSize = larger
                }
            } label: {
                Label("Increase Card Size", systemImage: "plus")
            }
            .disabled(cardSize.larger == nil)
            .help("Increase card size (\(ShortcutCatalog.increaseCardSize.display))")
        }
        .controlGroupStyle(.navigation)
        .labelStyle(.iconOnly)
        .accessibilityValue(cardSize.label)
        .accessibilityIdentifier(A11y.List.cardSizeControl)
        .onAppear {
            TestHooks.recordStartupEvent("toolbar")
        }
    }

    func focusSearch() {
        guard presentedReading == nil, !appState.isFocusMode else { return }
        CompactSearchToolbarConfiguration.beginSearchInteraction(in: NSApp.keyWindow)
        searchFocused = true
    }

    @ViewBuilder
    private var board: some View {
        if appState.isLoading || appState.isRestoringLibrary {
            Color.clear
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if appState.readings.isEmpty {
            emptyState
        } else {
            GeometryReader { proxy in
                LazyMasonryBoard(
                    appState.readings,
                    id: \.id,
                    minimumColumnWidth: cardSize.minimumColumnWidth,
                    spacing: Self.boardSpacing,
                    contentInsets: EdgeInsets(
                        top: Self.boardTopSpacing,
                        leading: Self.boardSpacing,
                        bottom: Self.boardSpacing,
                        trailing: Self.boardSpacing
                    ),
                    configurationID: cardSize.rawValue,
                    scrollResetID: appState.publishedBoardContext,
                    geometryKey: { AnyHashable($0.boardGeometryKey) },
                    position: $boardPosition,
                    navigationCoordinator: boardNavigation,
                    estimatedHeight: estimatedCardHeight,
                    content: { row in
                        OiaCardView(
                            row: row,
                            isSelected: appState.selectedIDs.contains(row.id),
                            cardSize: cardSize,
                            playbackPositions: videoPlaybackPositions,
                            viewportSize: proxy.size,
                            displayScale: displayScale,
                            scrollState: boardScrollState,
                            autoplayEnabled: presentedReading == nil,
                            reduceMotion: accessibilityReduceMotion,
                            scenePhase: scenePhase,
                            onSelect: {
                                select(
                                    row,
                                    extending: boardModifierKeys.contains(.shift)
                                )
                            },
                            onOpen: { open(row) },
                            onEditTags: { tagTargetID = row.id }
                        )
                        .environment(appState)
                        .accessibilityIdentifier(A11y.List.row(row.id))
                    }
                )
                .modifier(BoardScrollTrackingModifier(scrollState: boardScrollState))
                .focusable()
                .focused($boardFocused)
                .focusEffectDisabled()
                .onKeyPress(
                    keys: [.upArrow, .downArrow, .leftArrow, .rightArrow],
                    phases: [.down, .repeat],
                    action: moveSelection
                )
                .background {
                    BoardEscapeMonitor {
                        guard presentedReading == nil,
                              !appState.isEditingText,
                              !appState.boardSelection.isEmpty else { return false }
                        appState.boardSelection.clear()
                        return true
                    }
                }
                .onKeyPress(
                    keys: [
                        ShortcutCatalog.openWithReturn.key,
                        ShortcutCatalog.focusSearchWithSlash.key,
                        ShortcutCatalog.quickLook.primary.key
                    ],
                    phases: [.down],
                    action: performBoardShortcut
                )
                .onModifierKeysChanged(mask: .shift) { _, modifiers in
                    boardModifierKeys = modifiers
                }
                .simultaneousGesture(boardMagnifyGesture)
                .accessibilityIdentifier(A11y.List.table)
            }
        }
    }

    @ViewBuilder
    private var overlay: some View {
        if let row = presentedReading {
            OiaReadingOverlay(
                row: Binding(
                    get: { presentedReading ?? row },
                    set: updatePresentedRow
                ),
                rows: gallerySnapshot.rows,
                onClose: closeOverlay,
                onMove: moveOverlay,
                onSelect: open,
                canMovePrevious: canMoveOverlay(-1),
                canMoveNext: canMoveOverlay(1),
                onEditTags: { tagTargetID = row.id }
            )
        }
    }

    @ViewBuilder
    private var tagPicker: some View {
        if let row = tagTargetRow {
            TagPickerSheet(
                applied: row.tags,
                allTags: appState.filters.tags.map(\.tag),
                onToggle: { tag, shouldApply in
                    updateTag(tag, applies: shouldApply, to: row)
                }
            )
        } else {
            ContentUnavailableView("Item unavailable", systemImage: "exclamationmark.triangle")
                .frame(width: 380, height: 460)
        }
    }

    private var rowsProbe: some View {
        let ids = appState.readings.map(\.id)
        return Text(verbatim: "\(ids.count)")
            .foregroundStyle(.clear)
            .accessibilityIdentifier(A11y.List.rows)
            .accessibilityValue(ids.joined(separator: ","))
    }

    private var tagSheetPresented: Binding<Bool> {
        Binding(
            get: { tagTargetID != nil || appState.showTagSheet },
            set: { showing in
                if !showing {
                    tagTargetID = nil
                    appState.showTagSheet = false
                }
            }
        )
    }

    private var detailPresented: Binding<Bool> {
        Binding(
            get: { presentedReading != nil },
            set: { isPresented in
                if !isPresented {
                    closeOverlay()
                }
            }
        )
    }

    private var deleteDialogPresented: Binding<Bool> {
        Binding(
            get: { appState.pendingDelete != nil },
            set: {
                if !$0 {
                    appState.pendingDelete = nil
                }
            }
        )
    }

    private var errorAlertPresented: Binding<Bool> {
        Binding(
            get: { appState.error != nil },
            set: { showing in
                if !showing {
                    appState.error = nil
                }
            }
        )
    }

    private var tagTargetRow: ReadingRow? {
        let id = tagTargetID ?? (appState.showTagSheet ? appState.selectedId : nil)
        guard let id else { return nil }
        return appState.readings.first(where: { $0.id == id })
            ?? (presentedReading?.id == id ? presentedReading : nil)
    }

    private func updatePresentedRow(_ row: ReadingRow) {
        gallerySnapshot.update(row)
        presentedReading = row
    }

    func open(_ row: ReadingRow) {
        appState.selectReading(id: row.id, extending: false)

        if LibraryScope.links.contains(row) {
            if let url = row.sourceURL {
                ReadingLink.open(url)
            }
            return
        }

        if presentedReading == nil {
            gallerySnapshot = GallerySnapshot(appState.readings.filter { !LibraryScope.links.contains($0) })
        }
        boardFocused = false
        updatePresentedRow(row)
    }

    func closeOverlay() {
        presentedReading = nil
        gallerySnapshot = GallerySnapshot()
        appState.showHighlights = false
        if let id = appState.selectedId {
            boardPosition.scrollTo(id: id, anchor: .nearest)
        }
        boardFocused = true
    }

    private func moveOverlay(_ direction: Int) {
        guard let id = presentedReading?.id,
              let row = gallerySnapshot.neighbor(of: id, direction: direction) else { return }
        open(row)
    }

    private func canMoveOverlay(_ direction: Int) -> Bool {
        guard let id = presentedReading?.id else { return false }
        return gallerySnapshot.neighbor(of: id, direction: direction) != nil
    }

    private func updateTag(_ tag: String, applies: Bool, to row: ReadingRow) {
        if var presented = presentedReading, presented.id == row.id {
            if applies, !presented.tags.contains(tag) {
                presented.tags.append(tag)
            } else if !applies {
                presented.tags.removeAll { $0 == tag }
            }
            updatePresentedRow(presented)
        }
        Task {
            if applies {
                await appState.addTag(id: row.id, tag: tag)
            } else {
                await appState.removeTag(id: row.id, tag: tag)
            }
            if let refreshed = await appState.reloadRow(id: row.id), presentedReading?.id == row.id {
                updatePresentedRow(refreshed)
            }
        }
    }

    private func advanceOverlayPastCurrent() {
        if canMoveOverlay(1) {
            moveOverlay(1)
        } else if canMoveOverlay(-1) {
            moveOverlay(-1)
        } else {
            closeOverlay()
        }
    }
}
