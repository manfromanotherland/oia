// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct InspectorSource: View {
    let row: ReadingRow

    var body: some View {
        if let url = row.sourceURL {
            Button { ReadingLink.open(url) } label: {
                HStack(spacing: 4) {
                    Text(url.host() ?? row.displaySite ?? "Open source")
                        .lineLimit(1).truncationMode(.middle)
                    Image(systemName: "arrow.up.right").font(.system(size: 10, weight: .medium))
                }
            }
            .buttonStyle(.plain)
            .foregroundStyle(.primary)
            .font(.system(size: 12))
            .help(url.absoluteString)
            .contextMenu {
                Button("Copy Source URL") {
                    ReadingClipboard.copySourceURL(row)
                }
                .keyboardShortcut(ShortcutCatalog.copySourceURL)
            }
        } else {
            Text("Saved locally").font(.system(size: 12))
        }
    }
}

struct InspectorDetails: View {
    let row: ReadingRow
    let inspector: ReadingInspector?
    let failed: Bool
    var onSetImageType: ((ReadingImageType?) -> Void)?
    var automaticClassificationMessage: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Details")
                .font(InspectorTypography.sectionHeading)
                .padding(.bottom, 7)

            fact("Saved", ReadingDateTime.localized(row.savedAt))

            if row.kind == .image, let onSetImageType {
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    fieldLabel("Photo")
                    Spacer(minLength: 0)
                    Toggle("Photo", isOn: Binding(
                        get: { row.imageType == .photograph },
                        set: { onSetImageType($0 ? .photograph : .other) }
                    ))
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .help("Mark this image as a photo")
                    .accessibilityIdentifier(A11y.Inspector.imageType)
                }
                .padding(.vertical, 7)
                Button("Use automatic classification") { onSetImageType(nil) }
                    .buttonStyle(.plain)
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .padding(.bottom, 7)
                    .accessibilityIdentifier(A11y.Inspector.automaticImageType)
                if let automaticClassificationMessage {
                    Text(automaticClassificationMessage)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.bottom, 7)
                }
                Divider().opacity(0.65)
            }

            if row.isFullArticle, let readingTime = row.readingTimeLabel {
                fact("Reading time", readingTime)
            }
            if let file = inspector?.file {
                if let duration = file.durationMs {
                    fact("Duration", Self.duration(duration))
                }
                if let width = file.width, let height = file.height {
                    fact(row.kind == .article ? "Preview dimensions" : "Dimensions", "\(width) × \(height) px")
                }
                if !file.codecs.isEmpty {
                    fact("Codecs", file.codecs.joined(separator: ", "))
                }
                if let colorProfile = file.colorProfile {
                    fact("Color profile", colorProfile)
                }
                fact(row.kind == .article ? "Preview format" : "Format", file.format)
                fact(row.kind == .article ? "Preview size" : "Size", ByteCountFormatter.string(
                    fromByteCount: Int64(clamping: file.byteCount), countStyle: .file
                ))
            } else if inspector?.hasLocalFile == true {
                fact("File", "Unavailable")
            } else if failed {
                fact("File details", "Unavailable")
            }
            if let author = row.author, !author.isEmpty {
                fact("Author", author)
            }
            source
        }
        .accessibilityIdentifier(A11y.Inspector.details)
    }

    private func fact(_ label: String, _ value: String) -> some View {
        VStack(spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                fieldLabel(label)
                Spacer(minLength: 0)
                Text(value)
                    .font(.system(size: 12))
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.trailing)
                    .textSelection(.enabled)
            }
            .padding(.vertical, 7)
            Divider().opacity(0.65)
        }
    }

    private var source: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            fieldLabel("Source")
            Spacer(minLength: 0)
            InspectorSource(row: row)
        }
        .padding(.vertical, 7)
    }

    private func fieldLabel(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
    }

    private static func duration(_ milliseconds: UInt64) -> String {
        let seconds = milliseconds / 1000 + (milliseconds % 1000 >= 500 ? 1 : 0)
        let hours = seconds / 3600
        let minutes = (seconds % 3600) / 60
        let remaining = seconds % 60
        if hours > 0 {
            return String(format: "%02llu:%02llu:%02llu", hours, minutes, remaining)
        }
        return String(format: "%02llu:%02llu", minutes, remaining)
    }
}
