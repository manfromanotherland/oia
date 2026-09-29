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
            .font(.system(size: 12, weight: .semibold))
            .help(url.absoluteString)
            .contextMenu {
                Button("Copy source URL") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(url.absoluteString, forType: .string)
                }
            }
        } else {
            Text("Saved locally").font(.system(size: 12, weight: .semibold))
        }
    }
}

struct InspectorInformation: View {
    let row: ReadingRow
    let inspector: ReadingInspector?
    let failed: Bool
    @State private var showsMore = true

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Information")
                    .font(.system(size: 14, weight: .semibold))
                Spacer()
                Button(showsMore ? "Show Less" : "Show More") {
                    showsMore.toggle()
                }
                .buttonStyle(.plain)
                .foregroundStyle(.tint)
                .font(.system(size: 12, weight: .medium))
            }
            .padding(.bottom, 7)

            fact("Saved", Self.savedDate(row.savedAt))

            if showsMore {
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
        }
        .accessibilityIdentifier(A11y.Inspector.information)
    }

    private func fact(_ label: String, _ value: String) -> some View {
        VStack(spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                fieldLabel(label)
                Spacer(minLength: 0)
                Text(value)
                    .font(.system(size: 12, weight: .semibold))
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
            .font(.system(size: 12, weight: .semibold))
            .foregroundStyle(.secondary)
    }

    static func savedDate(_ value: String) -> String {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let date = fractional.date(from: value) ?? ISO8601DateFormatter().date(from: value)
        return date?.formatted(date: .abbreviated, time: .shortened) ?? value
    }

    private static func duration(_ milliseconds: UInt64) -> String {
        let seconds = milliseconds / 1_000 + (milliseconds % 1_000 >= 500 ? 1 : 0)
        let hours = seconds / 3_600
        let minutes = (seconds % 3_600) / 60
        let remaining = seconds % 60
        if hours > 0 {
            return String(format: "%02llu:%02llu:%02llu", hours, minutes, remaining)
        }
        return String(format: "%02llu:%02llu", minutes, remaining)
    }
}
