// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// A cheat sheet of every keyboard shortcut, grouped by area. Driven entirely by
/// `ShortcutCatalog`, so it always matches the live bindings. Opened with ⌘/.
struct ShortcutsView: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Keyboard Shortcuts")
                    .font(.headline)
                Spacer()
            }
            .padding(16)

            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 22) {
                    ForEach(ShortcutCatalog.groups) { group in
                        VStack(alignment: .leading, spacing: 10) {
                            Text(group.name.uppercased())
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(.secondary)
                            ForEach(group.shortcuts) { shortcut in
                                HStack(spacing: 12) {
                                    Text(shortcut.title)
                                    Spacer(minLength: 16)
                                    HStack(spacing: 4) {
                                        ForEach(shortcut.displays, id: \.self) { display in
                                            KeyCap(display)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                .padding(16)
            }

            Divider()

            HStack {
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier(A11y.Shortcuts.done)
            }
            .padding(12)
        }
        .frame(width: 420, height: 540)
        .accessibilityIdentifier(A11y.Shortcuts.sheet)
    }
}

/// A small key-cap rendering of a shortcut's symbols, e.g. ⇧⌘C.
private struct KeyCap: View {
    let symbols: String

    init(_ symbols: String) {
        self.symbols = symbols
    }

    var body: some View {
        Text(symbols)
            .font(.system(.body, design: .rounded).weight(.medium))
            .monospacedDigit()
            .padding(.horizontal, 9)
            .padding(.vertical, 4)
            .background(.quaternary, in: RoundedRectangle(cornerRadius: 6))
    }
}
