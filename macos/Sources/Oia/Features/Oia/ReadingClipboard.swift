// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// Copies a reading's local file or its web origin.
enum ReadingClipboard {
    static func fileURL(for row: ReadingRow, libraryURL: URL?) -> URL? {
        ReadingQuickLookURLResolver.previewURL(for: row, libraryURL: libraryURL)
    }

    static func copyFile(
        _ row: ReadingRow, libraryURL: URL?, to pasteboard: NSPasteboard = .general
    ) {
        guard let url = fileURL(for: row, libraryURL: libraryURL) else { return }
        pasteboard.clearContents()
        pasteboard.writeObjects([url as NSURL])
    }

    static func copySourceURL(_ row: ReadingRow, to pasteboard: NSPasteboard = .general) {
        guard let url = row.sourceURL else { return }
        pasteboard.clearContents()
        pasteboard.setString(url.absoluteString, forType: .string)
    }
}
