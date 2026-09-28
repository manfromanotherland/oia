// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

enum ReaderFont: String, CaseIterable, Identifiable {
    case system, serif, mono

    static let defaultChoice: ReaderFont = .serif
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

    /// Use the installed Palatino faces for Serif so SwiftUI and AppKit agree on
    /// the actual typeface rather than resolving the generic serif differently.
    func swiftUIFont(size: CGFloat, weight: Font.Weight = .regular,
                     bold: Bool = false, italic: Bool = false) -> Font
    {
        if self == .serif {
            return .custom(Self.serifFaceName(weight: weight, bold: bold, italic: italic), size: size)
        }
        var font = Font.system(size: size, design: design).weight(weight)
        if bold {
            font = font.bold()
        }
        if italic {
            font = font.italic()
        }
        return font
    }

    var id: String {
        rawValue
    }

    var label: String {
        switch self {
        case .system: "System"
        case .serif: "Serif"
        case .mono: "Monospace"
        }
    }

    /// SwiftUI font design used by the native reader.
    var design: Font.Design {
        switch self {
        case .system: .default
        case .serif: .serif
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
        CGFloat(rawValue)
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
        case .snug: 1.35
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
