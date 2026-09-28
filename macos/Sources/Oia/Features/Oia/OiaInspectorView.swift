// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Optional facts load independently of the board. The leading sidebar scrolls
/// its content while the reading remains visible beside it.
struct OiaInspectorView: View {
    @Environment(AppState.self) private var appState
    let row: ReadingRow
    var onEditTags: () -> Void
    var onSearch: (String) -> Void

    @State private var tab: InspectorTab = .discover
    @State private var inspector: ReadingInspector?
    @State private var loadedID: String?
    @State private var failed = false
    @State private var showsAnalysisInfo = false
    @State private var retry = 0

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                heading
                InspectorTabs(selection: $tab)
                if tab == .discover {
                    discover
                } else {
                    InspectorDetails(row: row, inspector: currentInspector, failed: failed)
                }
                Divider()
                Button(role: .destructive) {
                    appState.requestDelete(row)
                } label: {
                    Label("Delete", systemImage: "trash")
                }
                .buttonStyle(.plain)
                .foregroundStyle(.red)
                .disabled(appState.isDeleting)
                .accessibilityIdentifier(A11y.Toolbar.delete)
            }
            .font(.system(size: 13))
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollIndicators(.hidden)
        .scrollBounceBehavior(.basedOnSize)
        .accessibilityIdentifier(A11y.Inspector.panel)
        .task(id: loadID) { await load() }
    }

    private var heading: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(row.displayTitle)
                .font(.system(size: 16, weight: .semibold))
                .lineLimit(3)
                .help(row.displayTitle)
                .textSelection(.enabled)
            InspectorSource(row: row)
            if row.isFullArticle, let readingTime = row.readingTimeLabel {
                Label(readingTime, systemImage: "clock")
                    .labelStyle(.tightIcon)
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .help(row.wordCount.map { "\($0) words" } ?? "")
            }
            Text("Saved \(InspectorDetails.savedDate(row.savedAt))")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
    }

    private var discover: some View {
        VStack(alignment: .leading, spacing: 22) {
            tags
            if let data = currentInspector {
                if !row.isFullArticle, !data.colors.isEmpty { colors(data.colors) }
                if !data.labels.isEmpty { labels(data.labels) }
                if !data.analysisAvailable, row.previewAsset != nil {
                    status("Image attributes aren’t available yet.")
                } else if data.analysisAvailable, data.labels.isEmpty, data.colors.isEmpty {
                    status("No image attributes found.")
                }
            } else if failed {
                VStack(alignment: .leading, spacing: 8) {
                    status("Image attributes couldn’t be loaded.")
                    InspectorPill("Try again") { retry += 1 }
                }
            } else {
                ProgressView().controlSize(.small)
                    .accessibilityLabel("Loading image attributes")
            }
        }
    }

    private var tags: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                sectionTitle("Your tags")
                Spacer()
                InspectorPill(row.tags.isEmpty ? "Add" : "Edit", symbol: "plus", action: onEditTags)
                    .accessibilityLabel("Edit tags")
                    .accessibilityIdentifier(A11y.Inspector.editTags)
            }
            if row.tags.isEmpty {
                status("Add tags to make this yours.")
            } else {
                FlowLayout(spacing: 6) {
                    ForEach(row.tags, id: \.self) { tag in
                        InspectorPill(tag) { onSearch(tag) }
                            .help("Search for \(tag)")
                    }
                }
            }
        }
    }

    private func colors(_ colors: [InspectorColor]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle("Colours")
            HStack(spacing: 0) {
                ForEach(colors, id: \.hex) { color in
                    InspectorSwatch(color: color) { onSearch(color.searchQuery) }
                }
            }
            .padding(.leading, -3)
        }
    }

    private func labels(_ labels: [String]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 5) {
                sectionTitle("In this image")
                Button { showsAnalysisInfo.toggle() } label: {
                    Image(systemName: "info.circle")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("About image attributes")
                .help("About image attributes")
                .popover(isPresented: $showsAnalysisInfo) {
                    Text("""
                    Recognised on this Mac. Choose an attribute to search your library.
                    These suggestions can be imperfect and don’t change your tags.
                    """)
                    .font(.callout)
                    .padding(16)
                    .frame(width: 270)
                }
            }
            FlowLayout(spacing: 6) {
                ForEach(labels, id: \.self) { label in
                    InspectorPill(label.capitalized) { onSearch(label) }
                        .help("Search for \(label)")
                        .accessibilityIdentifier(A11y.Inspector.attribute(label))
                }
            }
        }
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text).font(.system(size: 12, weight: .semibold))
    }

    private func status(_ text: String) -> some View {
        Text(text).font(.system(size: 12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }

    private var currentInspector: ReadingInspector? {
        loadedID == loadID ? inspector : nil
    }

    private var loadID: String {
        [
            appState.libraryURL?.path ?? "", row.id,
            String(appState.libraryContentGeneration), String(appState.visualAnalysisGeneration), String(retry)
        ].joined(separator: ":")
    }

    private func load() async {
        let request = loadID
        let session = appState.librarySessionGeneration
        failed = false
        do {
            let data = try await appState.core?.getReadingInspector(id: row.id)
            guard !Task.isCancelled, request == loadID, session == appState.librarySessionGeneration else { return }
            inspector = data
            loadedID = request
            failed = data == nil
        } catch {
            guard !Task.isCancelled, request == loadID, session == appState.librarySessionGeneration else { return }
            loadedID = request
            inspector = nil
            failed = true
        }
    }
}
