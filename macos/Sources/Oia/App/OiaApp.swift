// SPDX-License-Identifier: GPL-3.0-or-later

import AppKit
import Sparkle
import SwiftUI

@main
struct OiaApp: App {
    @State private var appState = AppState()
    @AppStorage("appearanceMode", store: AppDefaults.store) private var appearanceMode: AppearanceMode = .system

    /// Sparkle remains bundled for the release pipeline, but stays dormant until
    /// Óia has an official appcast URL and signing key.
    private let updaterController: SPUStandardUpdaterController

    init() {
        // Single-window app: disable macOS automatic window tabbing so the
        // "New Tab", "Show Tab Bar", and "Move Tab to New Window" items never appear.
        NSWindow.allowsAutomaticWindowTabbing = false

        // There is no public Óia update feed yet, so background checks and
        // the manual update command remain disabled in every build.
        updaterController = SPUStandardUpdaterController(
            startingUpdater: false,
            updaterDelegate: nil,
            userDriverDelegate: nil
        )
    }

    var body: some Scene {
        WindowGroup("Óia") {
            ContentView()
                .environment(appState)
                .frame(minWidth: 900, minHeight: 600)
                .containerBackground(OiaTheme.shellSurface, for: .window)
                .onChange(of: appearanceMode, initial: true) { _, mode in
                    NSApplication.shared.appearance = mode.nsAppearance
                }
        }
        // Open roomy the first time (no saved frame yet); afterwards SwiftUI
        // restores the size the user left it at, so `.defaultSize` is ignored.
        .defaultSize(width: 1100, height: 720)
        .windowStyle(.titleBar)
        .windowToolbarStyle(.unified(showsTitle: false))
        .commands {
            CommandGroup(replacing: .newItem) {}
            ArticleCommands(appState: appState)
            TypographyCommands()
            AppearanceCommands(appearanceMode: $appearanceMode)
            NavigationCommands(appState: appState)
            HelpCommands(appState: appState)
        }

        Settings {
            SettingsView()
                .environment(appState)
        }
    }
}
