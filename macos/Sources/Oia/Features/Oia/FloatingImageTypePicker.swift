// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Native glass toggle buttons share one floating image filter surface.
struct FloatingImageTypePicker: View {
    @Binding var selection: ImageTypeFilter
    let unsureImageCount: Int

    var body: some View {
        styledButtons
            .controlSize(.extraLarge)
            .fixedSize()
            .modifier(FloatingImageTypeGlass())
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Image type")
            .accessibilityValue(effectiveSelection.label)
            .accessibilityIdentifier(A11y.Filter.imageType)
    }

    @ViewBuilder
    private var styledButtons: some View {
        if #available(macOS 26.0, *) {
            buttons.buttonStyle(.glass)
        } else {
            buttons.buttonStyle(.bordered)
        }
    }

    private var buttons: some View {
        HStack(spacing: 0) {
            ForEach(availableFilters) { filter in
                Toggle(
                    filter == .unsure ? "Unsure (\(count))" : filter.label,
                    isOn: selectionBinding(for: filter)
                )
                .toggleStyle(.button)
                .help("\(filter.label) (\(ShortcutCatalog.imageTypeFilterShortcut(for: filter).display))")
                .accessibilityLabel(accessibilityLabel(for: filter))
                .accessibilityAddTraits(effectiveSelection == filter ? [.isSelected] : [])
                .accessibilityIdentifier(A11y.Filter.imageType + "." + filter.rawValue)
            }
        }
    }

    private var count: Int { max(0, unsureImageCount) }

    var availableFilters: [ImageTypeFilter] {
        ImageTypeFilter.allCases.filter { $0 != .unsure || count > 0 }
    }

    private var effectiveSelection: ImageTypeFilter {
        selection.availableSelection(hasUnsureImages: count > 0)
    }

    /// Selecting the current native toggle cannot turn off the active filter.
    func selectionBinding(for filter: ImageTypeFilter) -> Binding<Bool> {
        Binding(
            get: { effectiveSelection == filter },
            set: { _ in selection = filter.availableSelection(hasUnsureImages: count > 0) }
        )
    }

    private func accessibilityLabel(for filter: ImageTypeFilter) -> String {
        guard filter == .unsure else { return filter.label }
        return "Unsure, \(count) \(count == 1 ? "image" : "images") to classify"
    }
}

private struct FloatingImageTypeGlass: ViewModifier {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    func body(content: Content) -> some View {
        let shape = Capsule()
        if reduceTransparency {
            content.background(Color(nsColor: .controlBackgroundColor), in: shape)
                .overlay(shape.strokeBorder(.primary.opacity(0.08)))
        } else if #available(macOS 26.0, *) {
            content.glassEffect(.regular, in: shape)
        } else {
            content.background(.regularMaterial, in: shape)
                .overlay(shape.strokeBorder(.primary.opacity(0.08)))
        }
    }
}
