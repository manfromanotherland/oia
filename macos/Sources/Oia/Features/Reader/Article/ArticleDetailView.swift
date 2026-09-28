// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct ArticleDetailView: View {
    // The reader's loading pipeline lives in `ArticleDetailView+Loading.swift`.
    // Swift's `private` is file-scoped, so the state that pipeline drives — and
    // only that state — is left at the default internal access rather than
    // `private`. Treat it as private to this view and its loading extension;
    // nothing else in the module touches it.
    @Environment(AppState.self) var appState
    var showsToolbar = true
    @AppStorage("readerFont", store: AppDefaults.store) private var readerFont: ReaderFont = .defaultChoice
    @AppStorage("readerFontSize", store: AppDefaults.store) private var readerFontSize: ReaderFontSize = .medium
    @AppStorage("readerWidth", store: AppDefaults.store) private var readerWidth: ReaderWidth = .medium
    @AppStorage("readerLineHeight", store: AppDefaults.store) private var readerLineHeight: ReaderLineHeight = .normal

    @State var row: ReadingRow?
    @State var articleDocument: ArticleDocument?
    @State var isLoading = false
    /// True when the selected reading's body is too large to render (see
    /// `maxParseBytes`); drives the oversize notice instead of the reader.
    @State var bodyTooLarge = false

    /// Parsed bodies of readings opened this session, so revisiting one shows
    /// instantly — no re-parse, no spinner (see `ArticleDocumentCache`).
    /// Highlights are deliberately not cached — they're an overlay reloaded on
    /// each open, so toggles made elsewhere still show on return.
    @State var cache = ArticleDocumentCache()

    /// Drives the full-screen image-zoom overlay: injected into the reader's
    /// environment so a clicked figure can raise the lightbox, and observed to
    /// present it over the whole detail pane (see `ImageLightbox`) and to drop the
    /// reader's own toolbar actions while it's up (see `toolbarItems`).
    @State private var imageZoom = ImageZoomPresenter()

    /// Bodies larger than this are not parsed at all — swift-markdown would
    /// freeze the main thread and spike memory on a pathological file. The
    /// reader shows an "open in browser" notice instead. ~10 MB is already
    /// ~1.5M words, far beyond any real article.
    let maxParseBytes = 10 * 1024 * 1024
    /// Word-count companion to `maxParseBytes`: a reading this long is treated as
    /// too large *without* fetching its body (see `load`). No real article nears
    /// 1M words, so this short-circuits a pathological body before the reader reads
    /// and marshals megabytes across the FFI.
    let maxParseWords: UInt32 = 1_000_000

    /// Reader typography, derived once from the persisted font settings and shared
    /// by the body (`MarkdownDocumentView`) and `ArticleHeaderView` so both rescale
    /// together.
    private var theme: MarkdownTheme {
        MarkdownTheme(font: readerFont, fontSize: readerFontSize,
                      width: readerWidth, lineHeight: readerLineHeight)
    }

    var body: some View {
        @Bindable var appState = appState
        Group {
            if appState.selectedId != nil {
                if isLoading {
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if let row {
                    articleView(row: row)
                } else if !isLoading {
                    // selectedId set but row not loaded yet — happens on first render
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            } else {
                emptyDetail
            }
        }
        // Reader figures raise the full-screen zoom; the lightbox layers over the
        // whole detail pane (see `imageZoomOverlay`). Navigating away dismisses it.
        .imageZoomOverlay(imageZoom)
        // The one trigger for loading: runs on appear, selection changes, and a
        // newer file-backed library generation. It cancels stale work while
        // avoiding the duplicate fetch caused by a separate `.onChange` task.
        // Navigating away also closes any open lightbox so it cannot linger.
        .task(id: contentLoadID) {
            let generation = appState.libraryContentGeneration
            let selectedID = appState.selectedId
            imageZoom.dismiss()
            await load(id: selectedID, contentGeneration: generation)
        }
        .toolbar { toolbarItems }
        // The reader's toolbar material stays hidden when the overlay supplies
        // its own chrome or a figure opens in the full-window lightbox. Otherwise
        // that material would cover the sidebar or the zoom backdrop at the top.
        .toolbarBackground(
            showsToolbar && imageZoom.target == nil ? .automatic : .hidden,
            for: .windowToolbar
        )
        .inspector(isPresented: $appState.showHighlights) {
            HighlightsInspector(readingId: appState.selectedId)
                .inspectorColumnWidth(min: 220, ideal: 280, max: 420)
        }
    }

    private var contentLoadID: String {
        "\(appState.selectedId ?? ""):\(appState.libraryContentGeneration)"
    }

    // ── Article content ───────────────────────────────────────────────────

    private func articleView(row: ReadingRow) -> some View {
        articleContent(row: row)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .navigationTitle(row.title.isEmpty ? "Article" : row.title)
    }

    /// The reader: the oversize notice, the parsed document, an excerpt-only
    /// fallback, or nothing. The article header is embedded inside the scroll
    /// area of each branch so it moves with the content.
    @ViewBuilder
    private func articleContent(row: ReadingRow) -> some View {
        if bodyTooLarge {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ArticleHeaderView(row: row, theme: theme)
                    OversizeNotice(url: row.sourceURL)
                }
            }
        } else if let articleDocument {
            reader(row: row, document: articleDocument)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                // Identity tied to the reading so switching articles builds a
                // fresh ScrollView (scrolled to top) instead of inheriting the
                // previous article's offset. Keyed on `row.id` — not the parsed
                // document — so background revalidation of the *same* reading
                // (which can swap in a re-parsed document) doesn't reset scroll.
                .id(row.id)
        } else if let excerpt = row.excerpt, !excerpt.isEmpty {
            excerptFallback(row: row, excerpt: excerpt)
                .id(row.id)
        } else {
            Spacer()
        }
    }

    /// The parsed reader itself, handed the reader's typography (face, size,
    /// measure, and leading) plus the header that scrolls with it.
    private func reader(row: ReadingRow, document: ArticleDocument) -> some View {
        MarkdownDocumentView(
            document: document,
            assetBaseURL: AssetImageLoader.readingFolderURL(
                libraryURL: appState.libraryURL, readingID: row.id
            ),
            font: readerFont,
            fontSize: readerFontSize,
            width: readerWidth,
            lineHeight: readerLineHeight,
            highlights: appState.highlights.map(\.text),
            onHighlight: { text in
                Task { await appState.toggleHighlight(id: row.id, text: text) }
            },
            header: { ArticleHeaderView(row: row, theme: theme) }
        )
    }

    /// Excerpt-only fallback when there's no parsed body to show.
    private func excerptFallback(row: ReadingRow, excerpt: String) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ArticleHeaderView(row: row, theme: theme)
                VStack(alignment: .leading, spacing: 0) {
                    Text(excerpt)
                        .foregroundStyle(.secondary)
                        .italic()
                        .lineSpacing(theme.lineSpacing)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxWidth: theme.contentMaxWidth, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .center)
                .padding(.horizontal, 20)
                .padding(.bottom, 80)
            }
        }
    }

    private var emptyDetail: some View {
        ContentUnavailableView("Select an item to view", systemImage: "doc.text")
            .accessibilityIdentifier(A11y.Detail.empty)
    }

    // ── Toolbar ───────────────────────────────────────────────────────────

    /// The selected row resolved straight from the shared, synchronously
    /// published `appState.readings` (kept fresh by every mutation's refresh).
    /// Driving the toolbar from this — rather than the async-loaded `@State row`
    /// — makes the actions appear/disappear in the same render frame as the rest
    /// of the selection-dependent toolbar. Using the async `row` instead lets the
    /// controls reflow a frame apart, which reads as a blink.
    private var currentRow: ReadingRow? {
        guard let id = appState.selectedId else { return nil }
        // Fall back to the loaded detail row when the selection sits outside the
        // current list, keeping the toolbar populated for what's still on screen.
        return appState.readings.first(where: { $0.id == id }) ?? (row?.id == id ? row : nil)
    }

    @ToolbarContentBuilder
    private var toolbarItems: some ToolbarContent {
        // The lightbox's backdrop is a content overlay and can't cover the unified
        // title bar, so the reader's own actions step aside while an image is
        // zoomed — they'd float above the dark backdrop and act on an article the
        // user can't see. Search, card size, and board filters remain available because
        // the zoom never covers the board toolbar.
        if showsToolbar, let row = currentRow, imageZoom.target == nil {
            ArticleToolbar(row: row, appState: appState)
        }
    }
}
