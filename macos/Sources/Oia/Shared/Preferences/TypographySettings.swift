// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import CoreText
import SwiftUI

enum ReaderFont: String, CaseIterable, Identifiable {
    case system, serif, mono, helveticaNeue, fraunces

    static let defaultChoice: ReaderFont = .helveticaNeue
    static let serifFamilyName = "Palatino"

    static func serifFaceName(weight: Font.Weight, bold: Bool = false, italic: Bool = false) -> String {
        let heavy = bold || weight == .semibold || weight == .bold
            || weight == .heavy || weight == .black
        return switch (heavy, italic) {
        case (true, true): "Palatino-BoldItalic"
        case (true, false): "Palatino-Bold"
        case (false, true): "Palatino-Italic"
        case (false, false): serifFamilyName
        }
    }

    /// Resolve the selected native face while preserving inline emphasis.
    func swiftUIFont(size: CGFloat, weight: Font.Weight = .regular,
                     bold: Bool = false, italic: Bool = false) -> Font
    {
        Font(nativeFont(size: size, weight: weight, bold: bold, italic: italic))
    }

    /// One font resolver for SwiftUI text and AppKit's selectable reader runs.
    func nativeFont(size: CGFloat, weight: Font.Weight = .regular,
                    bold: Bool = false, italic: Bool = false) -> NSFont
    {
        if self == .fraunces {
            return Self.frauncesFont(size: size, bold: bold || weight == .bold || weight == .semibold, italic: italic)
        }
        let fallback = NSFont.systemFont(ofSize: size, weight: weight.nsWeight)
        var descriptor = (self == .helveticaNeue
            ? NSFont(name: "HelveticaNeue", size: size) ?? fallback : fallback).fontDescriptor
        if self != .helveticaNeue, let designed = descriptor.withDesign(design.systemDesign) {
            descriptor = designed
        }
        var traits = descriptor.symbolicTraits
        if bold || weight == .bold || weight == .semibold {
            traits.insert(.bold)
        }
        if italic {
            traits.insert(.italic)
        }
        return NSFont(descriptor: descriptor.withSymbolicTraits(traits), size: size) ?? fallback
    }

    private static func frauncesFont(size: CGFloat, bold: Bool, italic: Bool) -> NSFont {
        let resource = italic ? "Fraunces-9ptBlackItalic" : "Fraunces-9ptBlack"
        guard let url = Bundle(for: ReaderFontBundleToken.self).url(forResource: resource, withExtension: "ttf"),
              let descriptors = CTFontManagerCreateFontDescriptorsFromURL(url as CFURL) as? [CTFontDescriptor],
              let base = descriptors.first
        else {
            assertionFailure("Bundled Fraunces font is missing")
            return NSFont.systemFont(ofSize: size)
        }
        // The exported playground uses normal CSS weight and automatic optical sizing.
        // This variable font's internal default is Black; explicitly select 400.
        let axes: [NSNumber: NSNumber] = [
            NSNumber(value: UInt32(0x7767_6874)): NSNumber(value: bold ? 700 : 400),
            NSNumber(value: UInt32(0x6F70_737A)): NSNumber(value: Double(size))
        ]
        let descriptor = CTFontDescriptorCreateCopyWithAttributes(
            base, [kCTFontVariationAttribute: axes] as CFDictionary
        )
        return CTFontCreateWithFontDescriptor(descriptor, size, nil) as NSFont
    }

    var id: String {
        rawValue
    }

    var label: String {
        switch self {
        case .system: "System"
        case .serif: "Serif"
        case .mono: "Monospace"
        case .helveticaNeue: "Helvetica Neue"
        case .fraunces: "Fraunces"
        }
    }

    /// SwiftUI font design used by the native reader.
    var design: Font.Design {
        switch self {
        case .system, .helveticaNeue: .default
        case .serif, .fraunces: .serif
        case .mono: .monospaced
        }
    }
}

enum ReaderFontSize: Int, CaseIterable, Identifiable {
    case small = 15, medium = 17, large = 19, xlarge = 21, huge = 23, giant = 25
    var id: Int {
        rawValue
    }

    var label: String {
        switch self {
        case .small: "Small"
        case .medium: "Medium"
        case .large: "Large"
        case .xlarge: "Extra Large"
        case .huge: "Huge"
        case .giant: "Giant"
        }
    }

    /// Base body point size for the native reader.
    var points: CGFloat {
        switch self {
        case .small: 16
        case .medium: 18
        case .large: 20
        case .xlarge: 22
        case .huge: 24
        case .giant: 26
        }
    }
}

/// How wide the reader lays the article out — the *measure*, in points.
///
/// Medium (680) is the long-standing default and sits at the ~60–75 character
/// sweet spot for body text; the neighbours trade measure for either a tighter
/// column on a large display or fuller use of a small one. Values are fixed
/// points rather than a multiple of the body size, so widening the text does not
/// silently widen the column too.
enum ReaderWidth: Int, CaseIterable, Identifiable {
    case xsmall = 520, small = 600, medium = 680, large = 800, xlarge = 960
    var id: Int {
        rawValue
    }

    var label: String {
        switch self {
        case .xsmall: "Extra Small"
        case .small: "Small"
        case .medium: "Medium"
        case .large: "Large"
        case .xlarge: "Extra Large"
        }
    }

    /// Maximum content width for the reader column.
    var points: CGFloat {
        CGFloat(rawValue)
    }
}

/// Leading between body lines, as a CSS-style line-height multiple of the body
/// size, in five stops from 1.2 to 2.0. Normal (1.5) is the middle stop.
///
/// The raw values are names, not numbers, so a stop can be retuned later without
/// resetting the choice every user has already saved — unlike `ReaderWidth`,
/// whose raw value *is* its measure.
enum ReaderLineHeight: String, CaseIterable, Identifiable {
    case tight, snug, normal, relaxed, loose
    var id: String {
        rawValue
    }

    var label: String {
        switch self {
        case .tight: "Tight"
        case .snug: "Snug"
        case .normal: "Normal"
        case .relaxed: "Relaxed"
        case .loose: "Loose"
        }
    }

    /// Total line height as a multiple of the body point size.
    var multiple: CGFloat {
        switch self {
        case .tight: 1.20
        case .snug: 1.475
        case .normal: 1.50
        case .relaxed: 1.75
        case .loose: 2.00
        }
    }

    /// What to add *between* lines to reach `multiple`. Both SwiftUI's
    /// `lineSpacing` and AppKit's `NSParagraphStyle.lineSpacing` are gaps layered
    /// on top of the font's natural (~1.2×) line height, so the extra leading is
    /// the difference — never negative, so Tight can't overlap lines.
    var extraLeadingMultiple: CGFloat {
        max(multiple - 1.2, 0)
    }
}

private final class ReaderFontBundleToken {}

// ── SwiftUI → AppKit mappings ────────────────────────────────────────────────

extension Font.Weight {
    /// Map the SwiftUI weights the theme uses onto AppKit font weights.
    var nsWeight: NSFont.Weight {
        if self == .ultraLight {
            return .ultraLight
        }
        if self == .thin {
            return .thin
        }
        if self == .light {
            return .light
        }
        if self == .medium {
            return .medium
        }
        if self == .semibold {
            return .semibold
        }
        if self == .bold {
            return .bold
        }
        if self == .heavy {
            return .heavy
        }
        if self == .black {
            return .black
        }
        return .regular
    }
}

extension Font.Design {
    var systemDesign: NSFontDescriptor.SystemDesign {
        switch self {
        case .serif: .serif
        case .rounded: .rounded
        case .monospaced: .monospaced
        default: .default
        }
    }
}
