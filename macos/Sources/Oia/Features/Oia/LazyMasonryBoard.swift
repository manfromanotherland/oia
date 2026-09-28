// SPDX-License-Identifier: GPL-3.0-or-later

import LazyLayoutKit
import SwiftUI

/// Óia' stable geometry contract for LazyLayoutKit. Every card receives a
/// deterministic height before its view is built, so scrolling never waits for
/// asset decoding or a SwiftUI measurement pass.
struct OiaMasonryLayout: LazyLayoutAlgorithm {
    typealias Item = ItemMetric

    let minimumColumnWidth: Double
    let spacing: Double
    let topInset: Double
    let leadingInset: Double
    let bottomInset: Double
    let trailingInset: Double

    func columnWidth(forContainerWidth containerWidth: Double) -> Double {
        let availableWidth = max(1, containerWidth - leadingInset - trailingInset)
        let gaps = spacing * Double(columnCount(forContainerWidth: containerWidth) - 1)
        return max(1, (availableWidth - gaps) / Double(columnCount(forContainerWidth: containerWidth)))
    }

    func layout(items: [ItemMetric], containerWidth: Double) -> LazyLayoutResult {
        let availableWidth = max(1, containerWidth - leadingInset - trailingInset)
        let masonry = LazyLayoutKit.MasonryLayout(
            columns: columnCount(forContainerWidth: containerWidth),
            spacing: spacing
        )
        let result = masonry.layout(items: items, containerWidth: availableWidth)
        let frames = result.frames.map { frame in
            LayoutRect(
                x: frame.x + leadingInset,
                y: frame.y + topInset,
                width: frame.width,
                height: frame.height
            )
        }
        return LazyLayoutResult(
            frames: frames,
            contentHeight: topInset + result.contentHeight + bottomInset
        )
    }

    func columnCount(forContainerWidth containerWidth: Double) -> Int {
        let availableWidth = max(1, containerWidth - leadingInset - trailingInset)
        let safeMinimum = minimumColumnWidth.isFinite && minimumColumnWidth > 0
            ? minimumColumnWidth : 1
        let divisor = safeMinimum + spacing
        guard divisor.isFinite, divisor > 0 else { return 1 }
        return max(1, Int((availableWidth + spacing) / divisor))
    }

    static func normalizedHeight(_ height: CGFloat) -> Double {
        guard height.isFinite, height > 0 else { return 180 }
        return Double(height)
    }
}

enum BoardNavigationDirection: Sendable {
    case upward
    case downward
    case leftward
    case rightward
}

/// Stable spatial navigation over the exact frames used by the masonry board.
/// Source order breaks ties so equal geometry never makes keyboard movement
/// nondeterministic.
struct MasonryNavigationIndex<ID: Hashable & Sendable>: Sendable {
    private struct Entry: Sendable {
        let id: ID
        let frame: LayoutRect
        let sourceIndex: Int
    }

    private let entries: [Entry]

    init(ids: [ID], frames: [LayoutRect]) {
        entries = zip(ids, frames).enumerated().compactMap { index, pair in
            let (id, frame) = pair
            guard Self.isValid(frame) else { return nil }
            return Entry(id: id, frame: frame, sourceIndex: index)
        }
    }

    func neighbor(of id: ID, toward direction: BoardNavigationDirection) -> ID? {
        guard let current = entries.first(where: { $0.id == id }) else { return nil }

        var best: (entry: Entry, score: [Double])?
        for candidate in entries where candidate.sourceIndex != current.sourceIndex {
            guard Self.isCandidate(candidate.frame, toward: direction, from: current.frame) else {
                continue
            }
            let score = Self.score(candidate, from: current, toward: direction)
            if best == nil || Self.precedes(score, best?.score ?? []) {
                best = (candidate, score)
            }
        }
        return best?.entry.id
    }

    private static func isValid(_ frame: LayoutRect) -> Bool {
        frame.x.isFinite && frame.y.isFinite
            && frame.width.isFinite && frame.height.isFinite
            && frame.width > 0 && frame.height > 0
    }

    private static func isCandidate(
        _ candidate: LayoutRect,
        toward direction: BoardNavigationDirection,
        from current: LayoutRect
    ) -> Bool {
        switch direction {
        case .upward:
            candidate.maxY <= current.minY
        case .downward:
            candidate.minY >= current.maxY
        case .leftward:
            candidate.maxX <= current.minX
        case .rightward:
            candidate.minX >= current.maxX
        }
    }

    private static func score(
        _ candidate: Entry,
        from current: Entry,
        toward direction: BoardNavigationDirection
    ) -> [Double] {
        let candidateFrame = candidate.frame
        let currentFrame = current.frame
        let horizontalGap = intervalGap(
            candidateFrame.minX, candidateFrame.maxX,
            currentFrame.minX, currentFrame.maxX
        )
        let verticalGap = intervalGap(
            candidateFrame.minY, candidateFrame.maxY,
            currentFrame.minY, currentFrame.maxY
        )
        let horizontalCenterDistance = abs(midX(candidateFrame) - midX(currentFrame))
        let verticalCenterDistance = abs(midY(candidateFrame) - midY(currentFrame))

        switch direction {
        case .upward, .downward:
            return [
                horizontalGap,
                verticalGap,
                horizontalCenterDistance,
                verticalCenterDistance,
                Double(candidate.sourceIndex)
            ]
        case .leftward, .rightward:
            return [
                horizontalGap,
                verticalGap,
                verticalCenterDistance,
                horizontalCenterDistance,
                Double(candidate.sourceIndex)
            ]
        }
    }

    private static func intervalGap(
        _ firstMin: Double,
        _ firstMax: Double,
        _ secondMin: Double,
        _ secondMax: Double
    ) -> Double {
        if firstMax < secondMin {
            return secondMin - firstMax
        }
        if secondMax < firstMin {
            return firstMin - secondMax
        }
        return 0
    }

    private static func midX(_ frame: LayoutRect) -> Double {
        frame.minX + frame.width / 2
    }

    private static func midY(_ frame: LayoutRect) -> Double {
        frame.minY + frame.height / 2
    }

    private static func precedes(_ lhs: [Double], _ rhs: [Double]) -> Bool {
        for (left, right) in zip(lhs, rhs) where left != right {
            return left < right
        }
        return lhs.count < rhs.count
    }
}

/// Spatial navigation consumes the exact snapshot already solved by the board.
/// It never measures cards or runs a second layout from a view body.
@MainActor
final class MasonryNavigationCoordinator<Element: Equatable, ID: Hashable & Sendable> {
    private var index = MasonryNavigationIndex<ID>(ids: [], frames: [])

    func update(snapshot: LayoutSnapshot<ID>) {
        index = MasonryNavigationIndex(ids: snapshot.ids, frames: snapshot.frames)
    }

    func neighbor(of id: ID, toward direction: BoardNavigationDirection) -> ID? {
        index.neighbor(of: id, toward: direction)
    }
}

/// A viewport-lazy masonry board backed by LazyLayoutKit. The complete reading
/// snapshot is cheap layout input; only cards in the materialized window become
/// SwiftUI views.
struct LazyMasonryBoard<Element: Equatable, ID: Hashable & Sendable>: View {
    @State private var visibility = BoardVisibilityCoordinator<ID>()

    private let elements: [Element]
    private let id: KeyPath<Element, ID>
    private let minimumColumnWidth: CGFloat
    private let spacing: CGFloat
    private let contentInsets: EdgeInsets
    private let configurationID: AnyHashable
    /// A new result context gets a fresh native scroll container. Ordinary
    /// mutations keep this stable so LazyLayoutKit can preserve their anchor.
    private let scrollResetID: AnyHashable
    private let geometryKey: ((Element) -> AnyHashable)?
    private let position: Binding<LazyLayoutPosition<ID>>?
    private let navigationCoordinator: MasonryNavigationCoordinator<Element, ID>?
    private let onViewportChange: ((LayoutRect) -> Void)?
    private let estimatedHeight: (Element, CGFloat) -> CGFloat
    private let content: (Element) -> AnyView

    init<Data>(
        _ data: Data,
        id: KeyPath<Element, ID>,
        minimumColumnWidth: CGFloat = 220,
        spacing: CGFloat = 18,
        contentInsets: EdgeInsets = .init(),
        configurationID: AnyHashable = 0,
        scrollResetID: AnyHashable = 0,
        geometryKey: ((Element) -> AnyHashable)? = nil,
        position: Binding<LazyLayoutPosition<ID>>? = nil,
        navigationCoordinator: MasonryNavigationCoordinator<Element, ID>? = nil,
        onViewportChange: ((LayoutRect) -> Void)? = nil,
        estimatedHeight: @escaping (Element, CGFloat) -> CGFloat = { _, _ in 180 },
        @ViewBuilder content: @escaping (Element) -> some View
    ) where Data: RandomAccessCollection, Data.Element == Element {
        let elements = Array(data)
        self.elements = elements
        self.id = id
        self.minimumColumnWidth = minimumColumnWidth
        self.spacing = spacing
        self.contentInsets = contentInsets
        self.configurationID = configurationID
        self.scrollResetID = scrollResetID
        self.geometryKey = geometryKey
        self.position = position
        self.navigationCoordinator = navigationCoordinator
        self.onViewportChange = onViewportChange
        self.estimatedHeight = estimatedHeight
        self.content = { AnyView(content($0)) }
    }

    var body: some View {
        let layout = OiaMasonryLayout(
            minimumColumnWidth: Double(minimumColumnWidth),
            spacing: Double(spacing),
            topInset: Double(contentInsets.top),
            leadingInset: Double(contentInsets.leading),
            bottomInset: Double(contentInsets.bottom),
            trailingInset: Double(contentInsets.trailing)
        )

        LazyLayoutView(
            elements,
            id: id,
            layout: layout,
            overscan: .items(80),
            position: position,
            recomputeOn: configurationID,
            geometryKey: geometryKey
        ) { element, containerWidth in
            .fixedHeight(
                OiaMasonryLayout.normalizedHeight(
                    estimatedHeight(
                        element,
                        CGFloat(layout.columnWidth(forContainerWidth: containerWidth))
                    )
                )
            )
        } content: { element in
            content(element)
                .environment(\.boardCardVisibility, visibility.tracker(for: element[keyPath: id]))
        }
        .preparingLayoutCooperatively()
        .onLayoutSnapshotChange { snapshot in
            navigationCoordinator?.update(snapshot: snapshot)
            visibility.update(snapshot: snapshot)
        }
        .onLayoutViewportChange { viewport in
            visibility.update(viewport: viewport)
            onViewportChange?(viewport)
        }
        .id(scrollResetID)
    }
}
