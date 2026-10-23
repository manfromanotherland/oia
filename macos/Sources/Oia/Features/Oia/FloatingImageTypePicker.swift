// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// A native image facet picker that floats independently of the board scopes.
struct FloatingImageTypePicker: View {
    @Binding var selection: ImageTypeFilter
    let unsureImageCount: Int

    var body: some View {
        styledPicker
            .labelsHidden()
            .controlSize(.extraLarge)
            .fixedSize()
            .modifier(FloatingImageTypeGlass())
            .accessibilityLabel("Image type")
            .accessibilityValue(availableSelection.wrappedValue.label)
            .accessibilityIdentifier(A11y.Filter.imageType)
    }

    @ViewBuilder
    private var styledPicker: some View {
        if #available(macOS 27.0, *) {
            picker.pickerStyle(.tabs)
        } else {
            picker.pickerStyle(.segmented)
        }
    }

    private var picker: some View {
        Picker("Image type", selection: availableSelection) {
            ForEach(availableFilters) { filter in
                Text(filter == .unsure ? "Unsure (\(count))" : filter.label)
                    .tag(filter)
                    .help("\(filter.label) (\(ShortcutCatalog.imageTypeFilterShortcut(for: filter).display))")
                    .accessibilityLabel(accessibilityLabel(for: filter))
                    .accessibilityIdentifier(A11y.Filter.imageType + "." + filter.rawValue)
            }
        }
    }

    private var count: Int { max(0, unsureImageCount) }

    private var availableFilters: [ImageTypeFilter] {
        ImageTypeFilter.allCases.filter { $0 != .unsure || count > 0 }
    }

    private var availableSelection: Binding<ImageTypeFilter> {
        Binding(
            get: { selection.availableSelection(hasUnsureImages: count > 0) },
            set: { selection = $0 }
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
