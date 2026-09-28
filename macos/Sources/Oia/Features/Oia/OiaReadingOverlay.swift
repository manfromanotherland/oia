// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct OiaReadingOverlay: View {
    @Environment(AppState.self) private var appState

    @Binding var row: ReadingRow
    var onClose: () -> Void
    var onMove: (Int) -> Void
    var canMovePrevious: Bool
    var canMoveNext: Bool
    var onEditTags: () -> Void

    @AppStorage("showsReadingInspector", store: AppDefaults.store) private var showsInspector = true

    var body: some View {
        readingContent
            .background(Color(nsColor: .windowBackgroundColor))
            .navigationTitle(row.displayTitle)
            .toolbar { detailToolbar }
            .focusedSceneValue(\.detailNavigationActions, detailNavigationActions)
            .onExitCommand {
                guard !appState.isEditingText else { return }
                onClose()
            }
    }

    private var readingContent: some View {
        HStack(alignment: .top, spacing: 0) {
            detail
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            if showsInspector {
                OiaInspectorView(row: row, onEditTags: onEditTags, onSearch: searchFromInspector)
                    .frame(width: 320)
                    .padding(.horizontal, 24)
                    .padding(.top, 24)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ToolbarContentBuilder
    private var detailToolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .navigation) {
            Button(action: onClose) {
                Label("Close Reading", systemImage: "xmark")
            }
            .help("Close reading (Escape)")
            .accessibilityIdentifier(A11y.Detail.close)
            .keyboardShortcut(.cancelAction)

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

        ToolbarItemGroup(placement: .primaryAction) {
            Button(action: onEditTags) {
                Label("Edit Tags", systemImage: "tag")
            }
            .help("Edit tags")
            .accessibilityIdentifier(A11y.Toolbar.tags)

            Menu {
                if let url = row.sourceURL {
                    Button {
                        ReadingLink.open(url)
                    } label: {
                        Label("Open Source", systemImage: "safari")
                    }
                    .keyboardShortcut(ShortcutCatalog.openInBrowser)
                }

                Button(role: .destructive) {
                    appState.requestDelete(row)
                } label: {
                    Label("Delete", systemImage: "trash")
                }
                .keyboardShortcut(ShortcutCatalog.delete)
                .disabled(appState.isDeleting)
            } label: {
                Label("More", systemImage: "ellipsis.circle")
            }
            .help("More actions")

            Button {
                showsInspector.toggle()
            } label: {
                Label(
                    showsInspector ? "Hide Inspector" : "Show Inspector",
                    systemImage: "sidebar.trailing"
                )
            }
            .help(showsInspector ? "Hide inspector" : "Show inspector")
            .accessibilityIdentifier(A11y.Inspector.toggle)
        }
    }

    private var detailNavigationActions: DetailNavigationActions {
        DetailNavigationActions(
            canMovePrevious: canMovePrevious,
            canMoveNext: canMoveNext,
            showsInspector: showsInspector,
            movePrevious: { onMove(-1) },
            moveNext: { onMove(1) },
            toggleInspector: { showsInspector.toggle() }
        )
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
