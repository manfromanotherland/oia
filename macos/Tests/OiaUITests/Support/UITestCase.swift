// SPDX-License-Identifier: GPL-3.0-or-later

import XCTest

/// Base class every UI test sits on. It gives each test an isolated temp library
/// (so tests never see each other's data) *and* an isolated `UserDefaults` suite
/// (so preference changes never touch the real `is.edmundo.cuttings` domain),
/// launches the app against both, and — on teardown — terminates the app and
/// destroys the temp library and its defaults suite. The dev's real library
/// bookmark and preferences are never touched.
class UITestCase: XCTestCase {
    /// The running app under test, set by a `launch*` call.
    private(set) var app: XCUIApplication!

    /// The isolated temp library the app was launched against; destroyed on teardown.
    private(set) var library: TestLibrary!

    // ── Lifecycle ───────────────────────────────────────────────────────────

    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
        app = nil
        library?.destroy() // also drops the isolated defaults suite
        library = nil
    }

    // ── Launch ────────────────────────────────────────────────────────────

    /// Launch straight into the main view against a fresh temp library seeded
    /// with `articles` (empty by default). The boot rebuild indexes them before
    /// the window appears.
    @discardableResult
    func launchApp(
        articles: [ArticleFixture] = [],
        configure: (inout LaunchOptions) -> Void = { _ in }
    ) throws -> XCUIApplication {
        let app = try launchAppWithoutWaiting(articles: articles, configure: configure)
        XCTAssertTrue(
            app.byId(A11y.List.rows).waitForExistence(timeout: 20),
            "Library board did not appear after launch."
        )
        return app
    }

    /// Launches the real app against a seeded temp library and returns as soon
    /// as its window exists, without waiting for library hydration to finish.
    /// Startup checks use this to inspect the first usable frame.
    @discardableResult
    func launchAppWithoutWaiting(
        articles: [ArticleFixture] = [],
        configure: (inout LaunchOptions) -> Void = { _ in }
    ) throws -> XCUIApplication {
        let library = try TestLibrary()
        try library.write(articles)
        self.library = library

        var options = LaunchOptions(libraryPath: library.libraryURL.path, dbPath: library.dbURL.path)
        options.defaultsSuite = library.defaultsSuiteName
        configure(&options)
        app = AppLauncher.launch(options)
        return app
    }

    /// Launch into onboarding (no library configured). `onboardingPickPath` is
    /// pointed at the temp library's folder, so clicking "Choose Library…"
    /// scaffolds *that* — keeping the flow testable without a system open panel.
    @discardableResult
    func launchOnboarding(configure: (inout LaunchOptions) -> Void = { _ in }) throws -> XCUIApplication {
        let library = try TestLibrary()
        self.library = library

        var options = LaunchOptions(
            dbPath: library.dbURL.path,
            onboardingPickPath: library.libraryURL.path
        )
        options.defaultsSuite = library.defaultsSuiteName
        configure(&options)
        app = AppLauncher.launch(options)
        XCTAssertTrue(
            app.byId(A11y.Onboarding.chooseLibrary).waitForExistence(timeout: 20),
            "Onboarding did not appear after launch."
        )
        return app
    }

    /// Terminates the running app and relaunches it against the **same** temp
    /// library, DB, and defaults suite, so a test can assert that persisted
    /// preferences survive a restart. Applies no defaults pins by default: the app
    /// must read back what it wrote (a pin would shadow the persisted value through
    /// the NSArgumentDomain).
    @discardableResult
    func relaunchApp(configure: (inout LaunchOptions) -> Void = { _ in }) -> XCUIApplication {
        let app = relaunchAppWithoutWaiting(configure: configure)
        XCTAssertTrue(
            app.byId(A11y.List.rows).waitForExistence(timeout: 20),
            "Library board did not appear after relaunch."
        )
        return app
    }

    /// Relaunches against the same files/index and returns when the window
    /// exists, allowing warm-start checks to observe the cached first frame.
    @discardableResult
    func relaunchAppWithoutWaiting(
        configure: (inout LaunchOptions) -> Void = { _ in }
    ) -> XCUIApplication {
        app?.terminate()
        var options = LaunchOptions(libraryPath: library.libraryURL.path, dbPath: library.dbURL.path)
        options.defaultsSuite = library.defaultsSuiteName
        configure(&options)
        app = AppLauncher.launch(options)
        return app
    }

    // ── Page objects ────────────────────────────────────────────────────────
    // Convenience accessors wrapping the running app. Use them only after a
    // `launch*` call.

    var onboarding: OnboardingPage {
        OnboardingPage(app: app)
    }

    var list: ReadingListPage {
        ReadingListPage(app: app)
    }

    var reader: ReaderPage {
        ReaderPage(app: app)
    }

    var tagInput: InspectorTagInputPage {
        InspectorTagInputPage(app: app)
    }

    var highlightsInspector: HighlightsPage {
        HighlightsPage(app: app)
    }

    var keyboard: Keyboard {
        Keyboard(app: app)
    }

    // ── On-disk assertions ────────────────────────────────────────────────

    /// Polls the on-disk article file for `id` until `predicate(frontmatter)`
    /// holds or `timeout` elapses — the "file is the source of truth" check after
    /// a mutation. Returns whether the predicate was satisfied in time.
    @discardableResult
    func waitForFrontmatter(
        id: String,
        timeout: TimeInterval = 8,
        _ predicate: (Frontmatter) -> Bool
    ) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if let frontmatter = library?.frontmatter(id: id), predicate(frontmatter) {
                return true
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        return false
    }

    /// Generic poll: waits until `condition()` holds or `timeout` elapses. For
    /// on-disk / cross-cutting assertions that aren't a single element or default.
    @discardableResult
    func wait(timeout: TimeInterval = 8, until condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if condition() {
                return true
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        return false
    }
}
