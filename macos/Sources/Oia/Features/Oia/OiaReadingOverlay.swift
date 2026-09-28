// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

struct OiaReadingOverlay: View {
    @Environment(AppState.self) private var appState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    @Binding var row: ReadingRow
    var onClose: () -> Void
    var onMove: (Int) -> Void
    var canMovePrevious: Bool
    var canMoveNext: Bool
    var onEditTags: () -> Void

    @AppStorage("showsReadingInspector", store: AppDefaults.store) private var showsInspector = true

    var body: some View {
        gallery
            .background(Color(nsColor: .windowBackgroundColor))
            .navigationTitle(row.displayTitle)
            .navigationBarBackButtonHidden(true)
            .toolbar { detailToolbar }
            .toolbarBackground(.hidden, for: .windowToolbar)
            .focusedSceneValue(\.detailNavigationActions, detailNavigationActions)
            .onExitCommand {
                guard !appState.isEditingText else { return }
                onClose()
            }
    }

    private var gallery: some View {
        HStack(alignment: .top, spacing: 0) {
            OiaInspectorView(
                row: row, isVisible: showsInspector,
                onEditTags: onEditTags, onSearch: searchFromInspector
            )
                .frame(width: 320)
                .frame(maxHeight: .infinity)
                .frame(width: showsInspector ? 320 : 0, alignment: .trailing)
                .clipped()
                .background {
                    Group {
                        if reduceTransparency {
                            OiaTheme.inspectorSidebarSurface
                        } else {
                            InspectorSidebarMaterial()
                        }
                    }
                    .ignoresSafeArea(.container, edges: .top)
                }
                .allowsHitTesting(showsInspector)
                .accessibilityHidden(!showsInspector)
            detail
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ToolbarContentBuilder
    private var detailToolbar: some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            Button {
                // The toolbar's click transaction can suppress content animations.
                // Toggle on the next main turn so the sidebar gets its own transaction.
                DispatchQueue.main.async { toggleInspector() }
            } label: {
                Label(
                    showsInspector ? "Hide Sidebar" : "Show Sidebar",
                    systemImage: "sidebar.leading"
                )
            }
            .help("\(showsInspector ? "Hide" : "Show") sidebar (\(ShortcutCatalog.toggleSidebar.display))")
            .accessibilityIdentifier(A11y.Inspector.toggle)
        }

        // Reserve the inspector's titlebar width without drawing glass over it.
        if #available(macOS 26.0, *) {
            ToolbarItem(placement: .navigation) {
                Color.clear.frame(width: 180, height: 1)
                    .accessibilityHidden(true)
            }
            .sharedBackgroundVisibility(.hidden)
        } else {
            ToolbarItem(placement: .navigation) {
                Color.clear.frame(width: 180, height: 1)
                    .accessibilityHidden(true)
            }
        }

        ToolbarItem(placement: .navigation) {
            Button(action: onClose) {
                Label("Close Detail", systemImage: "xmark")
            }
            .help("Close detail (Escape)")
            .accessibilityIdentifier(A11y.Detail.close)
            .keyboardShortcut(.cancelAction)
        }

        ToolbarItem(placement: .navigation) {
            previousNextControl
        }
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
                LocalReadingVideo(row: row, libraryURL: appState.libraryURL)
            } else {
                mediaDetail(showsPlay: true)
            }
        case .quote:
            OiaQuoteDetailView(row: row)
        }
    }

    private func mediaDetail(showsPlay: Bool) -> some View {
        ZStack {
            Color(nsColor: .windowBackgroundColor)
            LocalReadingImage(
                row: row, libraryURL: appState.libraryURL,
                fallbackAspectRatio: showsPlay ? 16 / 9 : 4 / 3,
                maxPixel: 3200, contentMode: .fit
            )
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

    private func searchFromInspector(_ query: String) {
        let value = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { return }
        // Palette matching and visual labels are distinct search token kinds.
        let nextQuery = ""
        let nextTokens = [BoardSearchToken.colorQuery(value)
            ?? BoardSearchToken(kind: .visual, value: value)]
        guard !nextTokens[0].value.isEmpty else { return }
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

private struct InspectorSidebarMaterial: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .sidebar
        view.blendingMode = .behindWindow
        view.state = .followsWindowActiveState
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {}
}

private struct OiaQuoteDetailView: View {
    @Environment(AppState.self) private var appState
    let row: ReadingRow

    @State private var bodyText: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                Text("“")
                    .font(.largeTitle)
                    .foregroundStyle(.secondary)
                    .frame(height: 46)

                Text(attributedQuote)
                    .font(.title)
                    .italic()
                    .lineSpacing(8)
                    .textSelection(.enabled)

                if let site = row.displaySite {
                    Text(site)
                        .font(.callout.weight(.medium))
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: 720, alignment: .leading)
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
