// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// The same system picker is used in the toolbar and the floating image filter.
/// Selection artwork, tracking, and animation remain entirely system-owned.
struct LibraryFilterPicker<Selection: Hashable, Content: View>: View {
    let title: String
    @Binding var selection: Selection
    @ViewBuilder let content: () -> Content

    var body: some View {
        Picker(title, selection: $selection, content: content)
            .pickerStyle(.segmented)
            .labelsHidden()
    }
}

struct FloatingImageTypePicker: View {
    @Binding var selection: ImageTypeFilter
    let unsureImageCount: Int

    var body: some View {
        if #available(macOS 26.0, *) {
            picker.glassEffect(.regular, in: .capsule)
        } else {
            picker
        }
    }

    private var picker: some View {
        LibraryFilterPicker(title: "Image type", selection: availableSelection) {
            ForEach(availableFilters) { filter in
                Text(label(for: filter))
                    .tag(filter)
                    .help("\(label(for: filter)) (\(ShortcutCatalog.imageTypeFilterShortcut(for: filter).display))")
                    .accessibilityIdentifier(A11y.Filter.imageType + "." + filter.rawValue)
            }
        }
        .controlSize(.extraLarge)
        .fixedSize()
        .accessibilityLabel("Image type")
        .accessibilityValue(selection.availableSelection(hasUnsureImages: unsureImageCount > 0).label)
        .accessibilityIdentifier(A11y.Filter.imageType)
    }

    var availableFilters: [ImageTypeFilter] {
        ImageTypeFilter.allCases.filter { $0 != .unsure || unsureImageCount > 0 }
    }

    private var availableSelection: Binding<ImageTypeFilter> {
        Binding(
            get: { selection.availableSelection(hasUnsureImages: unsureImageCount > 0) },
            set: { selection = $0.availableSelection(hasUnsureImages: unsureImageCount > 0) }
        )
    }

    private func label(for filter: ImageTypeFilter) -> String {
        filter == .unsure ? "Unsure (\(max(0, unsureImageCount)))" : filter.label
    }
}
