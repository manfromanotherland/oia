// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// Images opens in place, keeping its facets inside one visibly owned group.
/// A native Picker supplies the content height; the system toolbar supplies its
/// surrounding capsule so expansion never enlarges the control.
struct BoardScopeControl: View {
    @Binding var scope: LibraryScope
    @Binding var imageType: ImageTypeFilter
    let imageTypes: [ImageTypeFilter]

    @Environment(\.controlSize) private var controlSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @FocusState private var focusedSegment: String?

    var body: some View {
        NativeSegmentedHeightLayout {
            nativeHeightReference

            HStack(spacing: 0) {
                ForEach(LibraryScope.allCases) { candidate in
                    if candidate == .images && scope == .images {
                        imageGroup
                    } else {
                        scopeButton(candidate)
                    }
                }
            }
            .font(.system(size: labelSize, weight: .medium))
            .modifier(BoardScopeBackground())
        }
        .fixedSize()
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Filter")
        .accessibilityValue(selectionDescription)
        .accessibilityIdentifier(A11y.Filter.group)
        .onMoveCommand(perform: moveSelection)
        .animation(reduceMotion ? nil : .easeInOut(duration: 0.18), value: scope == .images)
    }

    private var nativeHeightReference: some View {
        Picker("Filter", selection: .constant(scope)) {
            ForEach(LibraryScope.allCases) { candidate in
                Text(candidate.label).tag(candidate)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .hidden()
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private var imageGroup: some View {
        HStack(spacing: 0) {
            ForEach(imageTypes) { filter in
                segment(
                    filter == .all ? "Images" : filter.label,
                    id: imageSegmentID(filter),
                    isSelected: imageType == filter
                ) {
                    imageType = filter
                }
                .help(filter == .all ? "Show all images" : "Show \(filter.label.lowercased())")
            }
        }
        .background(.primary.opacity(0.07), in: Capsule())
        .overlay(Capsule().strokeBorder(.primary.opacity(0.12), lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Image type")
        .accessibilityValue(imageType == .all ? "Images" : imageType.label)
        .accessibilityIdentifier(A11y.Filter.imageType)
    }

    private func scopeButton(_ candidate: LibraryScope) -> some View {
        segment(
            candidate.label,
            id: scopeSegmentID(candidate),
            isSelected: scope == candidate
        ) {
            scope = candidate
        }
        .help("Show \(candidate.label.lowercased())")
    }

    private func segment(
        _ label: String, id: String, isSelected: Bool, action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Text(label)
                .lineLimit(1)
                .padding(.horizontal, horizontalInset)
                .frame(maxHeight: .infinity)
                .contentShape(Capsule())
        }
        .buttonStyle(BoardScopeSegmentStyle(isSelected: isSelected, isFocused: focusedSegment == id))
        .focused($focusedSegment, equals: id)
        .accessibilityLabel(label)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
        .accessibilityIdentifier(id)
    }

    private var selectionDescription: String {
        scope == .images && imageType != .all ? "Images, \(imageType.label)" : scope.label
    }

    private var labelSize: CGFloat {
        switch controlSize {
        case .mini: NSFont.systemFontSize(for: .mini)
        case .small: NSFont.systemFontSize(for: .small)
        case .large: NSFont.systemFontSize(for: .large)
        default: NSFont.systemFontSize(for: .regular)
        }
    }

    private var horizontalInset: CGFloat {
        switch controlSize {
        case .mini: 8
        case .small: 10
        default: 14
        }
    }

    private func scopeSegmentID(_ candidate: LibraryScope) -> String {
        A11y.Filter.group + "." + candidate.rawValue
    }

    private func imageSegmentID(_ filter: ImageTypeFilter) -> String {
        filter == .all ? scopeSegmentID(.images) : A11y.Filter.imageType + "." + filter.rawValue
    }

    private enum Segment {
        case scope(LibraryScope)
        case image(ImageTypeFilter)
    }

    private var segments: [(id: String, value: Segment)] {
        LibraryScope.allCases.flatMap { candidate in
            if candidate == .images && scope == .images {
                return imageTypes.map { (imageSegmentID($0), Segment.image($0)) }
            }
            return [(scopeSegmentID(candidate), Segment.scope(candidate))]
        }
    }

    private func moveSelection(_ direction: MoveCommandDirection) {
        guard let focusedSegment,
              let index = segments.firstIndex(where: { $0.id == focusedSegment })
        else { return }
        let nextIndex: Int
        switch direction {
        case .left: nextIndex = index - 1
        case .right: nextIndex = index + 1
        default: return
        }
        guard segments.indices.contains(nextIndex) else { return }
        let next = segments[nextIndex]
        switch next.value {
        case .scope(let candidate): scope = candidate
        case .image(let filter): imageType = filter
        }
        self.focusedSegment = next.id
    }
}

/// The invisible native control contributes only height; visible content supplies
/// width. Both receive the same height with no extra vertical padding or insets.
private struct NativeSegmentedHeightLayout: Layout {
    func sizeThatFits(
        proposal: ProposedViewSize, subviews: Subviews, cache: inout ()
    ) -> CGSize {
        guard subviews.count == 2 else { return .zero }
        let height = subviews[0].sizeThatFits(.unspecified).height
        let content = subviews[1].sizeThatFits(ProposedViewSize(width: nil, height: height))
        return CGSize(width: content.width, height: height)
    }

    func placeSubviews(
        in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()
    ) {
        let size = ProposedViewSize(width: bounds.width, height: bounds.height)
        for subview in subviews {
            subview.place(at: bounds.origin, anchor: .topLeading, proposal: size)
        }
    }
}

private struct BoardScopeSegmentStyle: ButtonStyle {
    let isSelected: Bool
    let isFocused: Bool

    @Environment(\.colorScheme) private var colorScheme
    @State private var isHovered = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .foregroundStyle(.primary.opacity(isSelected ? 1 : 0.85))
            .background {
                if isSelected {
                    Capsule().fill(colorScheme == .dark ? .white.opacity(0.16) : .white.opacity(0.8))
                } else if isHovered || configuration.isPressed {
                    Capsule().fill(.primary.opacity(configuration.isPressed ? 0.12 : 0.06))
                }
            }
            .overlay {
                if isFocused {
                    Capsule().strokeBorder(Color.accentColor, lineWidth: 2)
                }
            }
            .onHover { isHovered = $0 }
    }
}

private struct BoardScopeBackground: ViewModifier {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    func body(content: Content) -> some View {
        if #available(macOS 26.0, *) {
            // The toolbar's shared background preserves the existing outer height.
            content
        } else if reduceTransparency {
            content.background(Color(nsColor: .controlBackgroundColor), in: Capsule())
                .overlay(Capsule().strokeBorder(.primary.opacity(0.12), lineWidth: 0.5))
        } else {
            content.background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(.primary.opacity(0.12), lineWidth: 0.5))
        }
    }
}
