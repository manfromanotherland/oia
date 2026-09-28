// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Finder-style filmstrip for the detail browser. The selected card stays visible
/// while keyboard commands or a click move through the frozen board order.
struct OiaGalleryStrip: View {
    @Environment(AppState.self) private var appState

    let rows: [ReadingRow]
    let selectedID: String
    let onSelect: (ReadingRow) -> Void

    var body: some View {
        VStack(spacing: 0) {
            Divider()

            ScrollViewReader { proxy in
                ScrollView(.horizontal) {
                    LazyHStack(spacing: 10) {
                        ForEach(rows) { row in
                            galleryButton(for: row)
                                .id(row.id)
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                }
                .scrollIndicators(.hidden)
                .onAppear {
                    proxy.scrollTo(selectedID, anchor: .center)
                }
                .onChange(of: selectedID) { _, id in
                    proxy.scrollTo(id, anchor: .center)
                }
            }
        }
        .frame(height: 112)
        .background(.regularMaterial)
    }

    private func galleryButton(for row: ReadingRow) -> some View {
        let selected = row.id == selectedID
        return Button {
            onSelect(row)
        } label: {
            GalleryThumbnail(row: row, libraryURL: appState.libraryURL)
                .frame(width: 82, height: 64)
                .clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                        .stroke(Color.primary.opacity(0.12), lineWidth: 1)
                }
                .padding(6)
                .background(
                    selected ? Color(nsColor: .selectedContentBackgroundColor).opacity(0.55) : .clear,
                    in: RoundedRectangle(cornerRadius: 9, style: .continuous)
                )
        }
        .buttonStyle(.plain)
        .help(row.displayTitle)
        .accessibilityLabel(row.displayTitle)
        .accessibilityValue(selected ? "Selected" : "")
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

private struct GalleryThumbnail: View {
    let row: ReadingRow
    let libraryURL: URL?

    var body: some View {
        ZStack {
            switch row.kind {
            case .image, .video:
                LocalReadingImage(
                    row: row,
                    libraryURL: libraryURL,
                    fallbackAspectRatio: 4 / 3,
                    maxPixel: 240,
                    contentMode: .fill
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .clipped()

                if row.kind == .video {
                    Image(systemName: "play.fill")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(6)
                        .background(.black.opacity(0.55), in: Circle())
                }
            case .article, .quote:
                documentThumbnail
            }
        }
        .background(Color(nsColor: .textBackgroundColor))
    }

    private var documentThumbnail: some View {
        VStack(alignment: .leading, spacing: 4) {
            Image(systemName: row.kind.symbol)
                .font(.caption)
                .foregroundStyle(.secondary)

            Text(row.displayTitle)
                .font(.system(size: 8, weight: .semibold))
                .foregroundStyle(.primary)
                .lineLimit(3)
                .multilineTextAlignment(.leading)

            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(7)
    }
}
