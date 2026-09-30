// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation
import XCTest

final class ReadingDateTimeTests: XCTestCase {
    func testFractionalAndWholeSecondTimestampsFollowLocaleClockAndTimeZone() {
        let us = Locale(identifier: "en_US")
        let uk = Locale(identifier: "en_GB")
        let utc = TimeZone(secondsFromGMT: 0)!
        let plusOne = TimeZone(secondsFromGMT: 3_600)!

        let fractional = "2026-08-20T12:40:04.000Z"
        let wholeSeconds = "2026-08-20T12:40:04Z"

        let usDate = ReadingDateTime.localized(fractional, locale: us, timeZone: utc)
        let ukDate = ReadingDateTime.localized(wholeSeconds, locale: uk, timeZone: utc)
        let localDate = ReadingDateTime.localized(fractional, locale: uk, timeZone: plusOne)

        XCTAssertTrue(usDate.contains("12:40"), usDate)
        XCTAssertTrue(usDate.contains("PM"), usDate)
        XCTAssertTrue(ukDate.contains("12:40"), ukDate)
        XCTAssertFalse(ukDate.contains("PM"), ukDate)
        XCTAssertTrue(localDate.contains("13:40"), localDate)
        XCTAssertFalse(usDate.contains("2026-08-20T"), usDate)
    }

    func testUnparseableTimestampKeepsItsSourceValue() {
        XCTAssertEqual(ReadingDateTime.localized("unknown"), "unknown")
    }
}
