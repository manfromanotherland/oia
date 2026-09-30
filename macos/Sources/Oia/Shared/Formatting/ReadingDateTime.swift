// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Turns timestamps stored in the library into dates for native UI surfaces.
/// The library keeps its original ISO 8601 value; formatting follows this device.
enum ReadingDateTime {
    static func localized(
        _ value: String,
        locale: Locale = .autoupdatingCurrent,
        timeZone: TimeZone = .autoupdatingCurrent
    ) -> String {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        guard let date = fractional.date(from: value) ?? ISO8601DateFormatter().date(from: value)
        else {
            return value
        }

        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.timeZone = timeZone
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }
}
