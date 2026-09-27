// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// Maps the app's library scopes onto the compatible core view enum. Legacy
/// core view cases stay exported so older clients and libraries remain readable.
extension LibraryScope {
    var ffiView: FfiView {
        switch self {
        case .all: .all
        case .images, .videos: .all
        case .articles: .articles
        case .links: .links
        case .quotes: .quotes
        }
    }

    /// The core's compatible view enum combines images and videos under
    /// `.media`. The board exposes them separately using its kind filter.
    var readingKindFilter: ReadingKind? {
        switch self {
        case .images: .image
        case .videos: .video
        case .all, .articles, .links, .quotes: nil
        }
    }
}
