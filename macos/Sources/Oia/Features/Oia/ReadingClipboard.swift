// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit

/// Copies the local file represented by a card.
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
}
