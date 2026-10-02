// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

struct OiaReadingOverlay: View {
    @Environment(AppState.self) private var appState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    @Binding var row: ReadingRow
    @Binding var showsInspector: Bool
    var onClose: () -> Void
    var onMove: (Int) -> Void
    var canMovePrevious: Bool
    var canMoveNext: Bool
    var tagInputFocusRequest: TagInputFocusRequest?

    var body: some View {
        gallery
            .background(Color(nsColor: .windowBackgroundColor))
            .navigationTitle(row.displayTitle)
            .navigationBarBackButtonHidden(true)
            .toolbarBackground(.hidden, for: .windowToolbar)
            .focusedSceneValue(\.detailNavigationActions, detailNavigationActions)
            .background {
                EscapeKeyMonitor {
                    guard !appState.isEditingText else { return false }
                    onClose()
                    return true
                }
            }
            .onExitCommand {
                guard !appState.isEditingText else { return }
                onClose()
            }
            .onChange(of: appState.readings.first { $0.id == row.id }) { _, updated in
                if let updated { row = updated }
            }
    }

    private var gallery: some View {
        NavigationSplitView(columnVisibility: inspectorVisibility) {
            OiaInspectorView(
                row: row, isVisible: showsInspector,
                tagInputFocusRequest: tagInputFocusRequest,
                onSearch: searchFromInspector,
                onToggleTag: updateTag
            )
                .navigationSplitViewColumnWidth(min: 280, ideal: 320, max: 440)
                .background {
                    Group {
                        if reduceTransparency {
                            OiaTheme.inspectorSidebarSurface
                        } else {
                            GalleryMaterial(material: .sidebar)
                        }
                    }
                    .ignoresSafeArea(.container, edges: .top)
                }
                .toolbar(removing: .sidebarToggle)
        } detail: {
            detail
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background {
                    if row.kind == .image || row.kind == .video {
                        mediaPanelSurface
                            .ignoresSafeArea(.container, edges: .top)
                    }
                }
                .toolbar {
                    if #available(macOS 26.0, *) {
                        ToolbarSpacer(.fixed, placement: .navigation)
                    }
                    ToolbarItem(placement: .navigation) {
                        closeButton
                    }
                    if #available(macOS 26.0, *) {
                        ToolbarSpacer(.fixed, placement: .navigation)
                    }
                    ToolbarItem(placement: .navigation) {
                        previousNextControl
                    }
                }
        }
        .navigationSplitViewStyle(.balanced)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var inspectorVisibility: Binding<NavigationSplitViewVisibility> {
        Binding(
            get: { showsInspector ? .all : .detailOnly },
            set: { showsInspector = $0 != .detailOnly }
        )
    }

    private var closeButton: some View {
        Button(action: onClose) {
            Label("Close Detail", systemImage: "xmark")
        }
        .labelStyle(.iconOnly)
        .help("Close detail (Escape)")
        .accessibilityIdentifier(A11y.Detail.close)
        .keyboardShortcut(.cancelAction)
        .accessibilityLabel("Close Detail")
    }

    private var previousNextControl: some View {
        ControlGroup("Navigate") {
            Button { onMove(-1) } label: {
                Label("Previous item", systemImage: "chevron.left")
            }
            .help("Previous item (Left Arrow or K)")
            .accessibilityIdentifier(A11y.Detail.previous)
            .keyboardShortcut(.leftArrow, modifiers: [])
            .disabled(!canMovePrevious || appState.isEditingText)

            Button { onMove(1) } label: {
                Label("Next item", systemImage: "chevron.right")
            }
            .help("Next item (Right Arrow or J)")
            .accessibilityIdentifier(A11y.Detail.next)
            .keyboardShortcut(.rightArrow, modifiers: [])
            .disabled(!canMoveNext || appState.isEditingText)
        }
        .controlGroupStyle(.navigation)
        .labelStyle(.iconOnly)
    }

    private var detailNavigationActions: DetailNavigationActions {
        DetailNavigationActions(
            canMovePrevious: canMovePrevious,
            canMoveNext: canMoveNext,
            showsInspector: showsInspector,
            movePrevious: { onMove(-1) },
            moveNext: { onMove(1) },
            toggleInspector: toggleInspector
        )
    }

    private func toggleInspector() {
        withAnimation(reduceMotion ? nil : .smooth(duration: 0.26)) {
            showsInspector.toggle()
        }
    }

    @ViewBuilder
    private var mediaPanelSurface: some View {
        if reduceTransparency {
            OiaTheme.previewPlaceholderBackground(for: row)
        } else {
            GalleryMaterial(material: .underWindowBackground)
                .overlay(OiaTheme.previewPlaceholderBackground(for: row).opacity(0.2))
        }
    }

    @ViewBuilder
    private var detail: some View {
        switch row.kind {
        case .article:
            if let profile = row.socialPostProfile {
                SocialPostDetailView(row: row, profile: profile)
            } else {
                ArticleDetailView(showsToolbar: false)
                    .background(Color(nsColor: .textBackgroundColor))
            }
        case .image:
            mediaDetail(showsPlay: false)
        case .video:
            if row.hasLocalVideoAsset {
                ZStack {
                    Color.clear
                    LocalReadingVideo(row: row, libraryURL: appState.libraryURL)
                        .aspectRatio(row.standaloneMediaAspectRatio ?? 16 / 9, contentMode: .fit)
                        .clipShape(OiaTheme.cardShape)
                        .overlay {
                            OiaTheme.cardShape
                                .stroke(OiaTheme.border, lineWidth: 1)
                                .allowsHitTesting(false)
                        }
                        .padding(38)
                }
            } else {
                mediaDetail(showsPlay: true)
            }
        case .quote:
            OiaQuoteDetailView(row: row)
        }
    }

    private func mediaDetail(showsPlay: Bool) -> some View {
        ZStack {
            Color.clear
            LocalReadingImage(
                row: row, libraryURL: appState.libraryURL,
                fallbackAspectRatio: row.standaloneMediaAspectRatio ?? (showsPlay ? 16 / 9 : 4 / 3),
                maxPixel: 3200, contentMode: .fit
            )
            .clipShape(OiaTheme.cardShape)
            .overlay {
                OiaTheme.cardShape
                    .stroke(OiaTheme.border, lineWidth: 1)
                    .allowsHitTesting(false)
            }
            .padding(38)

            if showsPlay {
                videoPlayGlyph
            }

            if showsPlay {
                videoSourceButton
            }
        }
    }

    private var videoPlayGlyph: some View {
        Circle()
            .fill(.black.opacity(0.64))
            .frame(width: 72, height: 72)
            .overlay {
                Image(systemName: "play.fill")
                    .font(.system(size: 25, weight: .semibold))
                    .foregroundStyle(.white)
                    .offset(x: 3)
            }
    }

    private func searchFromInspector(_ token: BoardSearchToken) {
        guard !token.displayValue.isEmpty else { return }
        // Every named chip in Tags narrows by the same effective tag set.
        let nextQuery = ""
        let nextTokens = [token]
        let searchChanged = appState.searchQuery != nextQuery
            || BoardSearchCriteria(tokens: appState.searchTokens)
            != BoardSearchCriteria(tokens: nextTokens)

        onClose()
        appState.activeScope = .all
        appState.searchQuery = nextQuery
        appState.searchTokens = nextTokens
        if !searchChanged {
            appState.searchDidChange()
        }
    }

    private func updateTag(_ tag: String, applies: Bool) {
        let id = row.id
        row.applyTagEdit(tag, applies: applies)
        Task {
            if applies {
                await appState.addTag(id: id, tag: tag)
            } else {
                await appState.removeTag(id: id, tag: tag)
            }
            if row.id == id, let refreshed = await appState.reloadRow(id: id) {
                row = refreshed
            }
        }
    }

    @ViewBuilder
    private var videoSourceButton: some View {
        if let url = row.sourceURL {
            Button("Open source page") {
                ReadingLink.open(url)
            }
            .buttonStyle(.borderedProminent)
            .tint(.white.opacity(0.18))
            .foregroundStyle(.white)
            .padding(24)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
        }
    }
}

private struct GalleryMaterial: NSViewRepresentable {
    let material: NSVisualEffectView.Material

    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = material
        view.blendingMode = .behindWindow
        view.state = .followsWindowActiveState
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {
        view.material = material
    }
}

private struct OiaQuoteDetailView: View {
    @Environment(AppState.self) private var appState
    let row: ReadingRow

    @State private var bodyText: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .center, spacing: 32) {
                quoteMark("“")

                Text(attributedQuote)
                    .font(Font(OiaCardTextMetrics.quoteDetailFont))
                    .foregroundStyle(.secondary)
                    .lineSpacing(OiaCardTextMetrics.quoteDetailLineSpacing)
                    .multilineTextAlignment(.center)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity)

                quoteMark("”")

                if let site = row.displaySite {
                    Text(site)
                        .font(.callout.weight(.medium))
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: 720)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 70)
            .padding(.vertical, 90)
        }
        .background(OiaTheme.cardTint(for: row.id))
        .task(id: contentLoadID) {
            let generation = appState.libraryContentGeneration
            let body = await appState.getBody(id: row.id)
            guard !Task.isCancelled,
                  generation == appState.libraryContentGeneration else { return }
            bodyText = body
        }
    }

    private func quoteMark(_ mark: String) -> some View {
        Text(mark)
            .font(Font(OiaCardTextMetrics.quoteDetailMarkFont))
            .foregroundStyle(.tertiary)
            .accessibilityHidden(true)
    }

    private var contentLoadID: String {
        "\(row.id):\(appState.libraryContentGeneration)"
    }

    private var displayText: String {
        let source: String = if let bodyText = bodyText?.trimmingCharacters(in: .whitespacesAndNewlines),
                                !bodyText.isEmpty
        {
            bodyText
        } else if let excerpt = row.excerpt, !excerpt.isEmpty {
            excerpt
        } else {
            row.displayTitle
        }
        return source.replacingOccurrences(
            of: #"(?m)^(?:\s*>\s?)+"#,
            with: "",
            options: [.regularExpression],
            range: source.startIndex ..< source.endIndex
        )
    }

    private var attributedQuote: AttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .inlineOnlyPreservingWhitespace
        )
        return (try? AttributedString(markdown: displayText, options: options))
            ?? AttributedString(displayText)
    }
}
