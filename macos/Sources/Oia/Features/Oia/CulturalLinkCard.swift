// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

/// Locally saved artwork carries the title; decoration identifies the entity
/// without adding a source footer or obscuring its cover.
struct CulturalLinkCard: View {
    let row: ReadingRow
    let category: ReadingLinkCategory
    let libraryURL: URL?
    let cardSize: CGSize
    let displayScale: CGFloat
    let isVisible: Bool
    let scrollState: BoardScrollState

    var body: some View {
        Group {
            if row.previewAsset == nil {
                missingArtwork
            } else {
                switch category {
                case .book: book
                case .music: music
                case .movie: movie
                }
            }
        }
        .frame(width: cardSize.width, height: cardSize.height)
    }

    private func artwork(size: CGSize) -> some View {
        LocalReadingImage(
            row: row, libraryURL: libraryURL,
            fallbackAspectRatio: category == .music ? 1 : (row.culturalCardAspectRatio ?? 2 / 3),
            maxPixel: AssetPreviewLoadPlan.displayMaxPixel(for: size, displayScale: displayScale),
            contentMode: .fit,
            loadsProgressively: true,
            isVisible: isVisible,
            scrollState: scrollState
        )
        .frame(width: size.width, height: size.height)
    }

    private var book: some View {
        artwork(size: cardSize)
            .background(OiaTheme.previewPlaceholderBackground(for: row))
            .overlay(alignment: .leading) {
                LinearGradient(
                    stops: [
                        .init(color: .black.opacity(0.34), location: 0),
                        .init(color: .white.opacity(0.22), location: 0.2),
                        .init(color: .black.opacity(0.13), location: 0.34),
                        .init(color: .clear, location: 1)
                    ],
                    startPoint: .leading, endPoint: .trailing
                )
                .frame(width: max(12, cardSize.width * 0.06))
                .accessibilityHidden(true)
            }
            .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
            .shadow(color: .black.opacity(0.14), radius: 5, x: 2, y: 3)
    }

    private var music: some View {
        ZStack(alignment: .bottom) {
            VinylRecord()
                .frame(width: cardSize.width * 0.88, height: cardSize.width * 0.88)
                .position(x: cardSize.width * 0.56, y: cardSize.width * 0.44)
                .accessibilityHidden(true)

            artwork(size: CGSize(width: cardSize.width, height: cardSize.width))
                .background(OiaTheme.previewPlaceholderBackground(for: row))
                .clipShape(RoundedRectangle(cornerRadius: 2, style: .continuous))
                .overlay {
                    Rectangle().stroke(.black.opacity(0.12), lineWidth: 0.5)
                }
                .shadow(color: .black.opacity(0.14), radius: 4, x: 0, y: 2)
        }
    }

    private var movie: some View {
        artwork(size: cardSize)
            .background(OiaTheme.previewPlaceholderBackground(for: row))
            .mask {
                FilmstripMask().fill(style: FillStyle(eoFill: true))
            }
    }

    private var missingArtwork: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label(category.label, systemImage: category.symbol)
                .font(.body.weight(.medium))
                .foregroundStyle(.secondary)
            Spacer(minLength: 0)
            Text(row.displayTitle)
                .font(.title2.weight(.semibold))
                .foregroundStyle(.primary)
                .lineLimit(6)
                .multilineTextAlignment(.leading)
        }
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
    }
}

private struct VinylRecord: View {
    var body: some View {
        Circle()
            .fill(RadialGradient(
                colors: [.black, Color(white: 0.16), .black],
                center: .center, startRadius: 0, endRadius: 160
            ))
            .overlay {
                Circle().fill(AngularGradient(
                    colors: [.clear, .white.opacity(0.12), .clear, .white.opacity(0.06), .clear],
                    center: .center
                ))
            }
            .overlay {
                Canvas { context, size in
                    let center = CGPoint(x: size.width / 2, y: size.height / 2)
                    for index in 0 ..< 18 {
                        let radius = size.width * (0.28 + Double(index) * 0.011)
                        let rect = CGRect(
                            x: center.x - radius, y: center.y - radius,
                            width: radius * 2, height: radius * 2
                        )
                        context.stroke(Path(ellipseIn: rect), with: .color(.white.opacity(0.06)), lineWidth: 0.5)
                    }
                    let label = size.width * 0.18
                    let rect = CGRect(x: center.x - label / 2, y: center.y - label / 2, width: label, height: label)
                    context.fill(Path(ellipseIn: rect), with: .color(Color(white: 0.24)))
                }
            }
    }
}

/// Transparent sprocket holes reveal the current light or dark board surface.
private struct FilmstripMask: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path(roundedRect: rect, cornerRadius: 8)
        let width = max(4, rect.width * 0.018)
        let height = max(6, rect.width * 0.026)
        let step = height + max(5, rect.width * 0.022)
        let inset = max(8, rect.width * 0.035)
        for vertical in stride(from: rect.minY + 10, through: rect.maxY - height - 10, by: step) {
            for horizontal in [rect.minX + inset, rect.maxX - inset - width] {
                path.addRoundedRect(
                    in: CGRect(x: horizontal, y: vertical, width: width, height: height),
                    cornerSize: CGSize(width: 2, height: 2)
                )
            }
        }
        return path
    }
}
