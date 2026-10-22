// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI
import UniformTypeIdentifiers

extension OiaLibraryView {
    var supportedDropTypes: [UTType] {
        [.fileURL, .movie, .image, .url, .plainText]
    }

    /// Finder copies videos as `public.file-url`, which is already accepted
    /// here. Advertising the broad `.movie` type to `onPasteCommand` prevents
    /// macOS from dispatching even ordinary text paste to this view, so inline
    /// movie representations remain a drop-only capability.
    var supportedPasteTypes: [UTType] {
        [.fileURL, .image, .url, .plainText]
    }

    var dropPrompt: some View {
        ZStack {
            Rectangle()
                .fill(.regularMaterial)

            VStack(spacing: 10) {
                Image(systemName: "square.and.arrow.down")
                    .font(.system(size: 34, weight: .medium))
                Text("Drop to save")
                    .font(.title2.weight(.semibold))
                Text("Links, text, images, videos, and .txt or .md files")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            .padding(28)
            .background(
                Color(nsColor: .windowBackgroundColor).opacity(0.92),
                in: RoundedRectangle(cornerRadius: 16, style: .continuous)
            )
            .overlay {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .stroke(Color.accentColor, lineWidth: 2)
            }
        }
        .ignoresSafeArea()
        .allowsHitTesting(false)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Drop to save links, text, images, videos, or text and Markdown files")
        .accessibilityIdentifier(A11y.Save.dropTarget)
    }

    func saveNotice(_ notice: SaveNotice) -> some View {
        Label(notice.message, systemImage: notice.systemImage)
            .font(.callout.weight(.medium))
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(.regularMaterial, in: Capsule())
            .overlay { Capsule().stroke(OiaTheme.border, lineWidth: 1) }
            .shadow(color: .black.opacity(0.12), radius: 12, y: 5)
            .padding(.bottom, appState.activeScope == .images && presentedReading == nil ? 64 : 24)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
            .allowsHitTesting(false)
            .accessibilityIdentifier(A11y.Save.notice)
    }

    func save(_ providers: [NSItemProvider]) {
        Task {
            let payloads = await IngestionItemProviderLoader.load(from: providers)
            let rejectedCount = max(0, providers.count - payloads.count)
            await appState.save(payloads, rejectedCount: rejectedCount)
        }
    }
}
