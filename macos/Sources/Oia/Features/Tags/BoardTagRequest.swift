// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Freeze the target cards while the tag editor is open, even if a background
/// refresh changes the board selection or search results.
struct BoardTagRequest: Identifiable {
    let id = UUID()
    let rows: [ReadingRow]
    let librarySession: UInt64
}
