// SPDX-License-Identifier: GPL-3.0-or-later

import SwiftUI

struct SettingsView: View {
    var body: some View {
        TabView {
            AppearanceSettingsTab()
                .tabItem { Label("Appearance", systemImage: "paintbrush") }

            TypographySettingsTab()
                .tabItem { Label("Typography", systemImage: "textformat") }

            LibrarySettingsTab()
                .tabItem { Label("Library", systemImage: "folder") }

            ExtensionsSettingsTab()
                .tabItem { Label("Extensions", systemImage: "puzzlepiece.extension") }
        }
        // Wide enough for the Typography tab's five-segment Width picker to show
        // "Extra Small" … "Extra Large" without truncating.
        .frame(width: 520)
        .padding(20)
    }
}

// ── Appearance ────────────────────────────────────────────────────────────────

private struct AppearanceSettingsTab: View {
    @AppStorage("appearanceMode", store: AppDefaults.store) private var appearanceMode: AppearanceMode = .system

    var body: some View {
        Form {
            Picker("Theme", selection: $appearanceMode) {
                ForEach(AppearanceMode.allCases) { mode in
                    Text(mode.label).tag(mode)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier(A11y.Settings.themePicker)
        }
        .formStyle(.grouped)
        .frame(minHeight: 120)
        .accessibilityIdentifier(A11y.Settings.appearanceTab)
    }
}

// ── Typography ────────────────────────────────────────────────────────────────

private struct TypographySettingsTab: View {
    @AppStorage("readerFont", store: AppDefaults.store) private var readerFont: ReaderFont = .defaultChoice
    @AppStorage("readerFontSize", store: AppDefaults.store) private var readerFontSize: ReaderFontSize = .medium
    @AppStorage("readerWidth", store: AppDefaults.store) private var readerWidth: ReaderWidth = .medium
    @AppStorage("readerLineHeight", store: AppDefaults.store) private var readerLineHeight: ReaderLineHeight = .normal

    var body: some View {
        Form {
            Picker("Font", selection: $readerFont) {
                ForEach(ReaderFont.allCases) { font in
                    Text(font.label).tag(font)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier(A11y.Settings.fontPicker)

            Picker("Size", selection: $readerFontSize) {
                ForEach(ReaderFontSize.allCases) { size in
                    Text(size.label).tag(size)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier(A11y.Settings.sizePicker)

            Picker("Width", selection: $readerWidth) {
                ForEach(ReaderWidth.allCases) { width in
                    Text(width.label).tag(width)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier(A11y.Settings.widthPicker)

            Picker("Line Height", selection: $readerLineHeight) {
                ForEach(ReaderLineHeight.allCases) { lineHeight in
                    Text(lineHeight.label).tag(lineHeight)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier(A11y.Settings.lineHeightPicker)

            // A live sample of every choice at once: the chosen face and size, set
            // to the chosen leading, inside a column scaled to the chosen measure.
            // Enough lines to make both the wrap width and the leading visible.
            Text(Self.previewText)
                .font(.custom(previewFontName, size: readerFontSize.points))
                .lineSpacing(readerFontSize.points * readerLineHeight.extraLeadingMultiple)
                .foregroundStyle(.secondary)
                .frame(maxWidth: previewWidth, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, 4)
                .accessibilityIdentifier(A11y.Settings.typographyPreview)
        }
        .formStyle(.grouped)
        .frame(minHeight: 260)
        .accessibilityIdentifier(A11y.Settings.typographyTab)
    }

    private static let previewText = """
    The quick brown fox jumps over the lazy dog, then \
    settles in to read a long article without straining.
    """

    /// The reader's measure, scaled down to fit the settings form. Shows the
    /// *relative* effect of each width — the real column is `readerWidth.points`.
    private var previewWidth: CGFloat {
        readerWidth.points * 0.45
    }

    private var previewFontName: String {
        switch readerFont {
        case .system: "-apple-system"
        case .serif: ReaderFont.serifFamilyName
        case .mono: "Menlo"
        }
    }
}

// ── Library ───────────────────────────────────────────────────────────────────

private struct LibrarySettingsTab: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        Form {
            LabeledContent("Location") {
                if let url = appState.libraryURL {
                    Text(url.path)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .truncationMode(.middle)
                } else {
                    Text("Not configured")
                        .foregroundStyle(.secondary)
                }
            }

            Button("Change Library…") {
                appState.chooseLibrary()
            }
            .disabled(!appState.canChangeLibrary)
            .accessibilityIdentifier(A11y.Settings.changeLibrary)

            if appState.libraryURL != nil {
                Section {
                    HStack {
                        Button("Open Inbox") { appState.openInbox() }
                        Button("Check Inbox") { appState.checkInbox() }
                            .disabled(appState.isProcessingInbox || appState.isReconcilingLibrary)
                    }
                    if let error = appState.inboxError {
                        Text(error)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    } else if appState.isProcessingInbox {
                        Text("Checking Inbox…")
                            .foregroundStyle(.secondary)
                    } else if appState.inboxPendingCount > 0 {
                        Text(appState.inboxPendingCount == 1
                            ? "1 item waiting for files to finish arriving."
                            : "\(appState.inboxPendingCount) items waiting for files to finish arriving.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(Array(appState.inboxIssues.prefix(3).enumerated()), id: \.offset) { _, issue in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(issue.name)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Text(issue.message)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    if appState.inboxIssues.count > 3 {
                        Text("\(appState.inboxIssues.count - 3) more items need attention.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                } header: {
                    Text("Inbox")
                } footer: {
                    Text("Shared files are saved to your library while Óia is open, then removed from Inbox. Files that need attention stay in Inbox.")
                }
            }
        }
        .formStyle(.grouped)
        .frame(minHeight: 120)
        .accessibilityIdentifier(A11y.Settings.libraryTab)
    }
}

// ── Extensions ────────────────────────────────────────────────────────────────

/// Browser extension availability. The native-messaging manifest is installed
/// automatically when the library boots (see `AppState.boot`); public store
/// actions remain unavailable until official Óia listings exist.
private struct ExtensionsSettingsTab: View {
    var body: some View {
        Form {
            Section {
                ExtensionStoreLinks()
            } header: {
                Text("Get the browser extension")
            } footer: {
                Text("Public Óia extension listings are coming soon.")
            }
        }
        .formStyle(.grouped)
        .frame(minHeight: 200)
        // Let users copy the links rather than retype them.
        .textSelection(.enabled)
        .accessibilityIdentifier(A11y.Settings.extensionsTab)
    }
}
