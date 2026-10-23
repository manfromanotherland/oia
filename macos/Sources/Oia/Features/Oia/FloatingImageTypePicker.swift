// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// One native selection control. AppKit owns tracking, focus, and accessibility;
/// native glass supplies the shared surface and the neutral selected lens.
struct FloatingImageTypePicker: NSViewRepresentable {
    @Binding var selection: ImageTypeFilter
    let unsureImageCount: Int

    func makeNSView(context: Context) -> ImageTypeSegmentedView {
        let view = ImageTypeSegmentedView()
        context.coordinator.view = view
        view.control.target = context.coordinator
        view.control.action = #selector(Coordinator.selectFilter(_:))
        updateNSView(view, context: context)
        return view
    }

    func updateNSView(_ view: ImageTypeSegmentedView, context: Context) {
        context.coordinator.parent = self
        view.update(selection: selection, unsureImageCount: unsureImageCount)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: ImageTypeSegmentedView, context: Context) -> CGSize? {
        nsView.intrinsicContentSize
    }

    func makeCoordinator() -> Coordinator { Coordinator(parent: self) }

    final class Coordinator: NSObject {
        var parent: FloatingImageTypePicker
        weak var view: ImageTypeSegmentedView?

        init(parent: FloatingImageTypePicker) { self.parent = parent }

        @objc func selectFilter(_ sender: NSSegmentedControl) {
            let filters = ImageTypeSegmentedView.filters(unsureImageCount: parent.unsureImageCount)
            guard filters.indices.contains(sender.selectedSegment) else { return }
            parent.selection = filters[sender.selectedSegment]
            // Update the lens immediately; persistence and board reconciliation
            // must not delay feedback from this native selection control.
            view?.needsLayout = true
            view?.layoutSubtreeIfNeeded()
        }
    }
}

final class ImageTypeSegmentedView: NSView {
    let control = NSSegmentedControl(labels: [], trackingMode: .selectOne, target: nil, action: nil)
    private let surface: NSView
    private let content = NSView()
    private let selectedLens: NSView?
    private static let height: CGFloat = 36

    override init(frame frameRect: NSRect) {
        if #available(macOS 26.0, *) {
            let glass = NSGlassEffectView()
            glass.style = .regular
            glass.cornerRadius = Self.height / 2
            glass.contentView = content
            surface = glass

            let lens = NSGlassEffectView()
            lens.style = .regular
            lens.cornerRadius = Self.height / 2
            selectedLens = lens
        } else {
            surface = content
            selectedLens = nil
        }
        super.init(frame: frameRect)

        control.segmentStyle = .automatic
        control.segmentDistribution = .fillEqually
        control.selectedSegmentBezelColor = nil
        if #available(macOS 26.0, *) {
            control.controlSize = .extraLarge
            // Replace the content-area bezel (and its dividers) with native
            // glass for the shared surface and the selected segment.
            (control.cell as? NSSegmentedCell)?.isBordered = false
        } else {
            control.controlSize = .large
        }
        control.setAccessibilityIdentifier(A11y.Filter.imageType)
        control.setAccessibilityLabel("Image type")
        addSubview(surface)
        if let selectedLens { content.addSubview(selectedLens) }
        content.addSubview(control)
    }

    required init?(coder: NSCoder) { nil }

    static func filters(unsureImageCount: Int) -> [ImageTypeFilter] {
        ImageTypeFilter.allCases.filter { $0 != .unsure || unsureImageCount > 0 }
    }

    func update(selection: ImageTypeFilter, unsureImageCount: Int) {
        let count = max(0, unsureImageCount)
        let filters = Self.filters(unsureImageCount: count)
        if control.segmentCount != filters.count { control.segmentCount = filters.count }
        for (index, filter) in filters.enumerated() {
            let label = filter == .unsure ? "Unsure (\(count))" : filter.label
            if control.label(forSegment: index) != label { control.setLabel(label, forSegment: index) }
            control.setToolTip(
                "\(label) (\(ShortcutCatalog.imageTypeFilterShortcut(for: filter).display))",
                forSegment: index
            )
        }
        let effectiveSelection = selection.availableSelection(hasUnsureImages: count > 0)
        control.selectedSegment = filters.firstIndex(of: effectiveSelection) ?? 0
        control.setAccessibilityValue(effectiveSelection.label)
        invalidateIntrinsicContentSize()
        needsLayout = true
    }

    override var intrinsicContentSize: NSSize {
        NSSize(width: control.intrinsicContentSize.width, height: Self.height)
    }

    override func layout() {
        super.layout()
        surface.frame = bounds
        content.frame = bounds
        control.frame = content.bounds
        guard let selectedLens, control.segmentCount > 0 else { return }
        let width = content.bounds.width / CGFloat(control.segmentCount)
        let index = userInterfaceLayoutDirection == .rightToLeft
            ? control.segmentCount - 1 - control.selectedSegment
            : control.selectedSegment
        selectedLens.frame = NSRect(x: CGFloat(index) * width, y: 0, width: width, height: bounds.height)
    }
}
