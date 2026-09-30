// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import SwiftUI

/// A website-authored preview color and the pure black/white text colour with
/// the stronger WCAG contrast against it. Core canonicalises `theme_color` as
/// `#rrggbb`; this parser remains defensive because library files are externally
/// writable and may come from an older or hand-edited source.
struct CardThemePalette: Equatable, Sendable {
    static let minimumTextContrast = 4.5

    let background: CardSRGBColor
    let foreground: CardSRGBColor

    init?(themeColor: String?) {
        guard let background = CardSRGBColor(hex: themeColor) else { return nil }
        self.init(background: background)
    }

    init(background: CardSRGBColor) {
        self.background = background
        let blackContrast = background.contrastRatio(with: .black)
        let whiteContrast = background.contrastRatio(with: .white)
        foreground = blackContrast >= whiteContrast ? .black : .white
    }

    var textContrast: Double {
        background.contrastRatio(with: foreground)
    }
}

struct CardSRGBColor: Equatable, Sendable {
    static let black = CardSRGBColor(red: 0, green: 0, blue: 0)
    static let white = CardSRGBColor(red: 1, green: 1, blue: 1)

    let red: Double
    let green: Double
    let blue: Double

    init?(hex rawValue: String?) {
        guard let value = rawValue?.trimmingCharacters(in: .whitespacesAndNewlines),
              value.count == 7,
              value.first == "#",
              value.dropFirst().allSatisfy(\.isHexDigit),
              let packed = UInt64(value.dropFirst(), radix: 16)
        else { return nil }

        red = Double((packed >> 16) & 0xFF) / 255
        green = Double((packed >> 8) & 0xFF) / 255
        blue = Double(packed & 0xFF) / 255
    }

    init(red: Double, green: Double, blue: Double) {
        self.red = red
        self.green = green
        self.blue = blue
    }

    var color: Color {
        Color(.sRGB, red: red, green: green, blue: blue, opacity: 1)
    }

    func contrastRatio(with other: CardSRGBColor) -> Double {
        let lighter = max(relativeLuminance, other.relativeLuminance)
        let darker = min(relativeLuminance, other.relativeLuminance)
        return (lighter + 0.05) / (darker + 0.05)
    }

    private var relativeLuminance: Double {
        0.2126 * Self.linearComponent(red)
            + 0.7152 * Self.linearComponent(green)
            + 0.0722 * Self.linearComponent(blue)
    }

    private static func linearComponent(_ component: Double) -> Double {
        component <= 0.04045
            ? component / 12.92
            : pow((component + 0.055) / 1.055, 2.4)
    }
}

enum OiaTheme {
    static let card = Color(nsColor: .controlBackgroundColor)
    static var cardShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: 8, style: .continuous)
    }
    static let shellSurface = semanticSurface(lightBlack: 0.08, darkBlack: 0.25)
    static let boardSurface = semanticSurface(lightBlack: 0.06, darkBlack: 0.12)
    static let inspectorSidebarSurface = semanticSurface(lightBlack: 0.10, darkBlack: 0.16)
    static let textCardSurface = semanticSurface(darkWhite: 0.05)
    static let xPostCardSurface = Color(nsColor: NSColor(name: nil) { appearance in
        let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        return isDark
            ? NSColor(srgbRed: 0x2B / 255, green: 0x30 / 255, blue: 0x35 / 255, alpha: 1)
            : NSColor(srgbRed: 0xE4 / 255, green: 0xE8 / 255, blue: 0xEA / 255, alpha: 1)
    })
    static let border = Color(nsColor: .separatorColor)

    private static func semanticSurface(
        lightBlack: CGFloat = 0,
        darkBlack: CGFloat = 0,
        darkWhite: CGFloat = 0
    ) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            let tint: NSColor = isDark && darkWhite > 0 ? .white : .black
            let amount = isDark ? (darkWhite > 0 ? darkWhite : darkBlack) : lightBlack
            var surface = NSColor.windowBackgroundColor
            appearance.performAsCurrentDrawingAppearance {
                let base = NSColor.windowBackgroundColor.usingColorSpace(.sRGB)
                    ?? NSColor.windowBackgroundColor
                surface = base.blended(withFraction: amount, of: tint) ?? base
            }
            return surface
        })
    }

    static func articlePalette(for row: ReadingRow) -> CardThemePalette? {
        guard row.kind == .article, !row.isSocialPost else { return nil }
        return CardThemePalette(themeColor: row.themeColor)
    }

    static func previewPalette(for row: ReadingRow) -> CardThemePalette? {
        if let color = row.dominantColor {
            return CardThemePalette(
                background: CardSRGBColor(red: color.red, green: color.green, blue: color.blue)
            )
        }
        return articlePalette(for: row)
    }

    static func previewBackgroundColor(for row: ReadingRow) -> CardSRGBColor? {
        previewPalette(for: row)?.background
    }

    static func previewPlaceholderBackground(for row: ReadingRow) -> Color {
        previewBackgroundColor(for: row)?.color ?? card
    }

    static func previewPlaceholderForeground(for row: ReadingRow) -> Color {
        previewPalette(for: row)?.foreground.color ?? .secondary
    }

    static func cardTint(for _: String) -> Color {
        card
    }
}

extension ReadingKind {
    var label: String {
        switch self {
        case .article: "Articles"
        case .image: "Images"
        case .quote: "Quotes"
        case .video: "Videos"
        }
    }

    var singularLabel: String {
        switch self {
        case .article: "Article"
        case .image: "Image"
        case .quote: "Quote"
        case .video: "Video"
        }
    }

    var symbol: String {
        switch self {
        case .article: "doc.text"
        case .image: "photo"
        case .quote: "quote.opening"
        case .video: "play.rectangle"
        }
    }
}

extension ReadingRow {
    var displayTitle: String {
        title.isEmpty ? url : title
    }

    /// Only web origins can be opened outside the app. Source-less paste/drop
    /// saves use a private `cuttings://local/...` identity so they remain
    /// content-addressed without leaking a machine-specific file path.
    var sourceURL: URL? {
        guard let url = URL(string: url),
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https"
        else { return nil }
        return url
    }

    var displaySite: String? {
        if let site, !site.isEmpty {
            return site
        }
        return sourceURL?.host?.replacingOccurrences(of: "www.", with: "")
    }
}
