// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct TypographyCommands: Commands {
    @AppStorage("readerFont", store: AppDefaults.store) private var readerFont: ReaderFont = .defaultChoice
    @AppStorage("readerFontSize", store: AppDefaults.store) private var readerFontSize: ReaderFontSize = .medium
    @FocusedValue(\.detailNavigationActions) private var detailNavigationActions

    var body: some Commands {
        CommandMenu("Typography") {
            Section("Font") {
                ForEach(ReaderFont.allCases) { font in
                    Button(font.label) { readerFont = font }
                        .disabled(readerFont == font)
                }
            }
            Section("Size") {
                Button("Increase Size") {
                    if let next = ReaderFontSize.allCases.first(where: { $0.rawValue > readerFontSize.rawValue }) {
                        readerFontSize = next
                    }
                }
                .keyboardShortcut(ShortcutCatalog.increaseFont)
                .disabled(
                    detailNavigationActions == nil
                        || readerFontSize == ReaderFontSize.allCases.last
                )

                Button("Decrease Size") {
                    if let prev = ReaderFontSize.allCases.last(where: { $0.rawValue < readerFontSize.rawValue }) {
                        readerFontSize = prev
                    }
                }
                .keyboardShortcut(ShortcutCatalog.decreaseFont)
                .disabled(detailNavigationActions == nil || readerFontSize == .small)

                Divider()

                ForEach(ReaderFontSize.allCases) { size in
                    Button(size.label) { readerFontSize = size }
                        .disabled(readerFontSize == size)
                }
            }
        }
    }
}
