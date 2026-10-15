// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

enum OriginalArticleNavigation {
    enum Decision: Equatable {
        case allow
        case openInBrowser
        case cancel
    }

    static func decide(
        destination: URL?, currentURL: URL?, isLink: Bool, targetsMainFrame: Bool?
    ) -> Decision {
        guard targetsMainFrame != false else { return .allow }
        guard let destination else { return .cancel }
        let scheme = destination.scheme?.lowercased()
        let isWebURL = (scheme == "https" || scheme == "http") && destination.host != nil

        if isLink {
            // Tables of contents and footnotes should scroll the embedded page.
            if isWebURL, targetsMainFrame == true, destination.fragment != nil,
               withoutFragment(destination) == currentURL.flatMap(withoutFragment)
            {
                return .allow
            }
            return isWebURL || scheme == "mailto" || scheme == "tel" ? .openInBrowser : .cancel
        }
        // Allow ordinary page loads and redirects, but not script-created windows.
        return isWebURL && targetsMainFrame != nil ? .allow : .cancel
    }

    private static func withoutFragment(_ url: URL) -> URL? {
        var components = URLComponents(url: url, resolvingAgainstBaseURL: true)
        components?.fragment = nil
        return components?.url
    }
}
