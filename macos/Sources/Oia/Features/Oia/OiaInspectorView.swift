// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Optional facts load independently of the board. The leading sidebar scrolls
/// its content while the reading remains visible beside it.
struct OiaInspectorView: View {
    @Environment(AppState.self) private var appState
    let row: ReadingRow
    let isVisible: Bool
    var onEditTags: () -> Void
    var onSearch: (BoardSearchToken) -> Void
    var onToggleTag: (String, Bool) -> Void

    @State private var inspector: ReadingInspector?
    @State private var loadedID: String?
    @State private var failed = false
    @State private var showsAnalysisInfo = false
    @State private var retry = 0

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                heading
                tags
                visualAnalysis
                InspectorDetails(row: row, inspector: currentInspector, failed: failed)
                Divider()
                Group {
                    if #available(macOS 26.0, *) {
                        deleteButton.buttonStyle(.glass)
                    } else {
                        deleteButton.buttonStyle(.bordered)
                    }
                }
                .buttonBorderShape(.capsule)
                .controlSize(.small)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .font(.system(size: 13))
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollIndicators(.hidden)
        .scrollBounceBehavior(.basedOnSize)
        .accessibilityIdentifier(A11y.Inspector.panel)
        .task(id: isVisible ? loadID : "") {
            guard isVisible else { return }
            await load()
        }
    }

    private var deleteButton: some View {
        Button(role: .destructive) {
            appState.requestDelete(row)
        } label: {
            Label("Delete", systemImage: "trash")
                .labelStyle(.titleAndIcon)
                .foregroundStyle(.red)
        }
        .disabled(appState.isDeleting)
        .accessibilityIdentifier(A11y.Toolbar.delete)
    }

    private var heading: some View {
        Text(row.displayTitle)
            .font(.system(size: 16, weight: .semibold))
            .lineLimit(3)
            .help(row.displayTitle)
            .textSelection(.enabled)
    }

    private var visualAnalysis: some View {
        VStack(alignment: .leading, spacing: 22) {
            if let data = currentInspector {
                if !row.isFullArticle, !data.colors.isEmpty { colors(data.colors) }
            } else if failed {
                VStack(alignment: .leading, spacing: 8) {
                    status("Image colours couldn’t be loaded.")
                    InspectorPill("Try again") { retry += 1 }
                }
            } else {
                ProgressView().controlSize(.small)
                    .accessibilityLabel("Loading image colours")
            }
        }
    }

    private var tags: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                sectionTitle("Tags")
                Button { showsAnalysisInfo.toggle() } label: {
                    Image(systemName: "info.circle")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("About tags")
                .popover(isPresented: $showsAnalysisInfo) {
                    Text("""
                    Blue tags are yours. Purple tags are generated locally from the saved content.
                    Edit or right-click a tag to remove it. Removed machine tags stay removed.
                    """)
                    .font(.callout)
                    .padding(16)
                    .frame(width: 280)
                }
                Spacer()
                InspectorPill(row.tags.isEmpty ? "Add" : "Edit", symbol: "plus", action: onEditTags)
                    .accessibilityLabel("Edit tags")
                    .accessibilityIdentifier(A11y.Inspector.editTags)
            }
            if row.tags.isEmpty {
                status("Add your own tags.")
            } else {
                FlowLayout(spacing: 6) {
                    ForEach(row.tags, id: \.self) { tag in
                        tagPill(tag)
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
                    InspectorSwatch(color: color) { onSearch(BoardSearchToken(kind: .color, value: color.searchQuery)) }
                }
            }
            .padding(.leading, -3)
        }
    }

    private func tagPill(_ tag: String) -> some View {
        let isMachine = row.machineTags.contains { ExactTagIdentity.matches($0, tag) }
        return InspectorPill(tag, symbol: isMachine ? "sparkles" : nil, tint: isMachine ? .purple : .blue) {
            onSearch(BoardSearchToken(kind: .tag, value: tag))
        }
        .help("\(isMachine ? "Machine" : "Your") tag · Search for \(tag)")
        .accessibilityLabel("\(tag), \(isMachine ? "machine tag" : "your tag")")
        .accessibilityIdentifier(A11y.Inspector.attribute(tag))
        .contextMenu {
            if isMachine {
                Button("Make this my tag", systemImage: "person") {
                    onToggleTag(tag, true)
                }
            }
            Button("Remove tag", systemImage: "xmark", role: .destructive) {
                onToggleTag(tag, false)
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
