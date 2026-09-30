// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct InspectorPill: View {
    let title: String
    var symbol: String?
    var tint: Color?
    var action: () -> Void

    init(_ title: String, symbol: String? = nil, tint: Color? = nil, action: @escaping () -> Void) {
        self.title = title
        self.symbol = symbol
        self.tint = tint
        self.action = action
    }

    var body: some View {
        Group {
            if #available(macOS 26.0, *) {
                button.buttonStyle(.glass)
            } else {
                button.buttonStyle(.bordered)
            }
        }
        .buttonBorderShape(.capsule)
        .controlSize(.small)
        .font(.system(size: 12, weight: .medium))
        .tint(tint ?? .accentColor)
        .foregroundStyle(tint ?? .primary)
        .fixedSize(horizontal: true, vertical: false)
    }

    private var button: some View {
        Button(action: action) {
            HStack(spacing: 4) {
                if let symbol { Image(systemName: symbol).font(.system(size: 10, weight: .semibold)) }
                Text(title).lineLimit(1).truncationMode(.middle).frame(maxWidth: 232)
            }
            .padding(.horizontal, 3)
            .padding(.vertical, 2)
        }
    }
}

struct InspectorSwatch: View {
    let color: InspectorColor
    var action: () -> Void

    @State private var isHovered = false

    private var fill: Color { Color(red: color.red, green: color.green, blue: color.blue) }

    var body: some View {
        Button(action: action) {
            Circle().fill(fill).frame(width: 30, height: 30)
                .overlay {
                    Circle().strokeBorder(.primary.opacity(isHovered ? 0.35 : 0.12), lineWidth: 1)
                }
                .frame(width: 36, height: 38)
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
        .help("Search similar colours · \(color.hex)")
        .accessibilityLabel("Search colours similar to \(color.hex)")
        .accessibilityIdentifier(A11y.Inspector.colorPrefix + color.hex)
    }
}
