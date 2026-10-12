// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct InspectorPill: View {
    let title: String
    var symbol: String?
    var action: () -> Void

    init(_ title: String, symbol: String? = nil, action: @escaping () -> Void) {
        self.title = title
        self.symbol = symbol
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
        .fixedSize(horizontal: true, vertical: false)
    }

    private var button: some View {
        Button(action: action) {
            HStack(spacing: 4) {
                if let symbol {
                    Image(systemName: symbol).font(.system(size: 10, weight: .semibold))
                }
                Text(title).lineLimit(1).truncationMode(.middle).frame(maxWidth: 232)
            }
            .padding(.horizontal, 3)
            .padding(.vertical, 2)
        }
    }
}

/// Search and removal share one glass capsule. A fixed icon slot keeps the
/// label in place when hover changes the symbol.
struct InspectorTagPill: View {
    let title: String
    let symbol: String
    var onSearch: () -> Void
    var onRemove: () -> Void

    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 4) {
            Button(action: onRemove) {
                Image(systemName: isHovered ? "xmark.circle.fill" : symbol)
                    .font(.system(size: 10, weight: .semibold))
                    .frame(width: 12, height: 12)
                    .padding(.leading, 8)
                    .padding(.vertical, 6)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Remove \(title) tag")
            .accessibilityLabel("Remove \(title) tag")
            .accessibilityIdentifier(A11y.Inspector.removeTag(title))

            Button(action: onSearch) {
                Text(title).lineLimit(1).truncationMode(.middle).frame(maxWidth: 232)
                    .padding(.trailing, 8)
                    .padding(.vertical, 5)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Search for \(title)")
            .accessibilityLabel("Search for \(title)")
        }
        .font(.system(size: 12, weight: .medium))
        .modifier(InspectorTagGlass())
        .fixedSize(horizontal: true, vertical: false)
        .onHover { isHovered = $0 }
        .accessibilityElement(children: .contain)
    }
}

/// A read-only acknowledgement of a Tag added from the board. Its capsule
/// shares the inspector's presentation without offering search or removal.
struct InspectorTagSummaryPill: View {
    let title: String

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "tag")
                .font(.system(size: 10, weight: .semibold))
                .frame(width: 12, height: 12)
            Text(title)
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: 232)
        }
        .font(.system(size: 12, weight: .medium))
        .padding(.horizontal, 8)
        .padding(.vertical, 5)
        .modifier(InspectorTagGlass(isInteractive: false))
        .fixedSize(horizontal: true, vertical: false)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(title) tag")
    }
}

private struct InspectorTagGlass: ViewModifier {
    var isInteractive = true

    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    func body(content: Content) -> some View {
        let shape = Capsule()
        if reduceTransparency {
            content.background(Color(nsColor: .controlBackgroundColor), in: shape)
                .overlay(shape.strokeBorder(.primary.opacity(0.08)))
        } else if #available(macOS 26.0, *) {
            content.glassEffect(isInteractive ? .regular.interactive() : .regular, in: shape)
        } else {
            content.background(.regularMaterial, in: shape)
                .overlay(shape.strokeBorder(.primary.opacity(0.08)))
        }
    }
}

struct InspectorSwatch: View {
    let color: InspectorColor
    var action: () -> Void

    @State private var isHovered = false

    private var fill: Color {
        Color(red: color.red, green: color.green, blue: color.blue)
    }

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
