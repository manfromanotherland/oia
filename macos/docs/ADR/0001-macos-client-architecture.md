# ADR 0001: macOS Client Architecture

## Status

Accepted.

## Context

`macos` is the native SwiftUI client for **Óia**, a local-first visual
library. It embeds `core` through UniFFI.

The Rust core is the owner of the product domain:

- library format and frontmatter semantics,
- parsing and writing article files,
- file-first mutations,
- SQLite indexing and FTS search,
- smart-view filtering,
- tag, rating, state, highlight, and delete behavior.

The macOS app should stay a thin native client. Its job is to compose the macOS
experience, call the core, watch the filesystem, manage selection and optimistic
UI, and render Markdown natively. It must not grow a second copy of the domain
model or business rules in Swift.

The current app has already outgrown a flat demo structure: it has a central
`AppState`, a `CoreBridge`, platform services, sidebar/list/reader/settings UI,
Markdown rendering, tags, ratings, highlights, commands, and preferences. We need
a folder structure and naming convention that make ownership obvious.

## Decision

Use a feature-oriented SwiftUI architecture with a central app store.

The macOS app will be organized around these responsibilities:

- `core` owns durable domain entities and business rules.
- `CoreBridge` owns the async Swift boundary to Rust.
- `AppState` owns UI orchestration: selected reading, active filters, search,
  complete board snapshots, optimistic mutations, sidebar counts, refreshes, sheets, and
  inspectors.
- Feature folders own SwiftUI rendering and small feature-local presentation
  helpers.
- Platform folders own macOS APIs such as security-scoped bookmarks, FSEvents,
  native-host installation, and AppKit focus/editing integration.
- Shared folders own reusable UI components, formatters, and per-device
  preferences.

Do not introduce a parallel Swift domain layer with entities such as
`Reading`, `Tag`, or `Highlight` unless they are clearly presentation snapshots
or adapters. Persisted product truth belongs in Rust and in the library files.

Naming rule: presentation models drop the `Ffi` prefix (`FfiReadingRow` →
`ReadingRow`). `Ffi*` types must not appear outside `Bridge/`. All names follow
the shared glossary (`UBIQUITOUS_LANGUAGE.md`).

## Target Folder Organization

The target shape for `macos/Sources/Oia/` is:

```text
Sources/Oia/
  App/
    OiaApp.swift
    Commands/

  Bridge/
    CoreBridge.swift
    Mappers/

  Platform/
    Library/
    NativeHost/
    TextEditing/

  State/
    AppState/
    SidebarItem.swift
    SidebarCounts.swift
    ReadingSort.swift

  Features/
    Shell/
    Sidebar/
    ReadingList/
    Reader/
      Article/
      Markdown/
    Tags/
    Highlights/
    Settings/
    Shortcuts/

  Shared/
    Components/
    Formatting/
    Preferences/

  Resources/
```

This is a target organization, not a requirement to move every file in one
large refactor. Prefer incremental moves that keep behavior unchanged.

## Folder Responsibilities

### App

Application entry point and app-level command wiring.

Examples:

- `OiaApp.swift`
- menu command definitions,
- keyboard shortcut command integration,
- app lifecycle setup.

`App` should not contain feature UI or domain behavior.

### Bridge

Swift boundary to `core`.

Examples:

- `CoreBridge.swift`,
- UniFFI adapter helpers,
- mapping between generated FFI DTOs and Swift app-facing snapshots.

`Bridge` may know about `FfiReadingRow`, `FfiHighlight`, and other generated
types. Feature views should not need to know that data came from FFI.

(The glossary defines Core as "the Rust engine", so this folder is named
`Bridge`, not `Engine` — a Swift `Engine` would wrongly imply domain logic
lives in Swift.)

### Platform

macOS-specific services and system integration.

Examples:

- security-scoped library bookmarks (`LibraryBookmark`),
- library folder scaffolding (`LibrarySetup`),
- FSEvents folder watching (`FolderWatcher` — the glossary's "folder
  watcher"),
- native messaging host manifest installation (`NativeHostInstaller`),
- AppKit text-editing/focus integration (`TextEditingMonitor`).

`Platform` code can use AppKit/Foundation APIs directly. It should not decide
product semantics such as what counts as unread, archived, or favorite.

### State

Application orchestration and UI state.

Examples:

- `AppState`,
- `AppState+Library`,
- `AppState+Readings`,
- `AppState+Mutations`,
- `AppState+Highlights`,
- sidebar selection and count state (`SidebarItem`, `SidebarCounts`),
- reading query order (`ReadingSort`).

`State` owns UI coordination. It can call `CoreBridge`, hold snapshots returned
from the core, and apply optimistic UI changes. It must reconcile back to the
core/index rather than treating Swift state as persisted truth.

### Features

User-facing SwiftUI feature areas.

Feature folders own views and feature-local presentation helpers:

- `Shell`: top-level app frame, onboarding versus main app, split-view
  composition, app-wide sheets/dialogs/inspectors.
- `Sidebar`: smart views (All, Unread, Read, Archive, Favorites), ratings,
  tags, counts, sidebar navigation.
- `ReadingList`: reading rows, board snapshot loading, empty list states.
- `Reader`: article detail, article toolbar, rating footer, Markdown renderer.
- `Tags`: tag picker and tag editing UI.
- `Highlights`: highlights inspector and highlight UI.
- `Settings`: appearance, typography, library, and native-host settings.
- `Shortcuts`: keyboard shortcuts help UI.

Features render state and send user intents back to `AppState`. They should not
call generated UniFFI objects directly unless the file is explicitly an adapter.

### Shared

Reusable Swift code that is not tied to one feature.

Examples:

- small reusable controls,
- formatting helpers such as reading-time display,
- per-device preferences such as appearance and typography settings,
- layout helpers used by more than one feature.

Keep `Shared` small. If a helper is only used by one feature, keep it inside that
feature folder.

### Resources

Bundled app resources, fixtures, welcome content, and asset catalogs.

## Model Policy

The app has three kinds of models.

### 1. Core Entities

Owned by Rust.

Examples:

- `Reading`,
- `ReadingMetadata`,
- `LibraryRoot`,
- `Tag`,
- `Highlight`,
- rating and status mutation rules,
- list/search query semantics.

Swift must not duplicate the rules for these entities. Swift can ask the core
for snapshots and can submit intents back to the core.

### 2. Boundary DTOs

Generated or exposed through UniFFI.

Examples:

- `FfiReadingRow`,
- `FfiHighlight`,
- `FfiTagCount`,
- `FfiRatingCount`,
- `FfiListOptions`.

These names are acceptable at the `Bridge` boundary. They should not spread
deeply into SwiftUI feature code over time.

### 3. Presentation Models

Swift-owned app-facing snapshots or UI state.

Examples:

- `ReadingRow`,
- `HighlightRow`,
- `TagCount`,
- `RatingCount`,
- `SidebarItem`,
- `ReadingSort`,
- `AppearanceSettings`,
- `TypographySettings`.

Presentation models can be used freely by features. They are snapshots, not
persisted truth.

## ReadingRow Decision

Swift feature code should prefer `ReadingRow` over `FfiReadingRow`.

`FfiReadingRow` says "this value came from the Rust FFI boundary." That is useful
inside `Bridge`, but noisy and leaky inside SwiftUI views.

`ReadingRow` says "this value is a list/search/sidebar presentation snapshot of
a reading." The glossary defines the reading list as a list of *reading rows*,
so the name is the domain term; it is intentionally not the full `Reading`
entity.

`ReadingRow` mirrors `FfiReadingRow`'s fields exactly and as `var`. Mirroring
keeps every view's field access unchanged, so adopting it is a rename plus a
thin mapper rather than a field-by-field rework. `var` (not `let`) is required
because the optimistic-UI edits in `AppState.Mutations` copy-and-tweak a row
(`var updated = row; updated.read.toggle()`). `Sendable` lets the snapshot flow
through the `@MainActor` app state without concurrency friction.

```swift
struct ReadingRow: Identifiable, Equatable, Sendable {
    var id: String
    var title: String
    var url: String
    var canonicalUrl: String
    var author: String?
    var site: String?
    var savedAt: String
    var read: Bool
    var archived: Bool
    var favorite: Bool
    var rating: UInt8
    var excerpt: String?
    var wordCount: UInt32?
    var lang: String?
    var tags: [String]
}
```

Mapping happens in `Bridge/Mappers`, not inside leaf views. It is field-for-field
today; the mapper is the seam for any future presentation-only derivation (e.g.
parsing `savedAt` into a `Date`), so views never touch the boundary type. The
init lives in an extension so the memberwise initializer stays synthesized:

```swift
extension ReadingRow {
    init(_ row: FfiReadingRow) {
        id = row.id
        title = row.title
        url = row.url
        canonicalUrl = row.canonicalUrl
        author = row.author
        site = row.site
        savedAt = row.savedAt
        read = row.read
        archived = row.archived
        favorite = row.favorite
        rating = row.rating
        excerpt = row.excerpt
        wordCount = row.wordCount
        lang = row.lang
        tags = row.tags
    }
}
```

The shape follows the generated bindings: `FfiReadingRow` exposes `savedAt` as an
ISO-8601 `String` and has no `readAt` (date sorting is done in the core, not on
the row), so `ReadingRow` carries `savedAt: String` and omits `readAt` rather
than inventing fields the boundary can't fill.

## Sharing Core Data Across Features

Use reading identity and core-backed snapshots.

Preferred flow:

```text
core
  owns persisted Reading and business rules

CoreBridge
  exposes async Swift methods

AppState
  holds UI snapshots and selected reading id

Features
  render snapshots and send intents/actions
```

Features should share `readingId` as the primary currency. A `ReadingRow`
snapshot is fine for display, but mutations should be expressed as intents:

```swift
await appState.toggleFavorite(id: row.id)
await appState.archive(id: row.id)
await appState.addTag(id: row.id, tag: tag)
await appState.setRating(id: row.id, rating: 5)
```

This avoids multiple features each owning mutable copies of the same reading.

## Mutation Rule

All persisted reading mutations go through the core.

Swift may optimistically update presentation state immediately, but the core
call and follow-up refresh are authoritative.

Do:

```text
user action
  -> AppState applies optimistic presentation update
  -> AppState calls CoreBridge
  -> Rust core writes file first and syncs index
  -> AppState refreshes from core/index
```

Do not:

```text
feature view writes local state as truth
feature view writes database directly
feature view edits article files directly
Swift duplicates frontmatter mutation rules
```

## Rejected Alternatives

### Full Clean Architecture In Swift

Rejected for now.

The Rust core already acts as the domain/application layer. Adding Swift
repositories, use cases, and domain entities would create duplicated ownership
and more files without solving the current problem.

### View Model Per Feature As The Primary Architecture

Deferred.

Small feature-local view models are fine, especially for isolated UI like
settings or sheets. But the main sidebar/list/reader experience has shared
selection, filters, counts, optimistic removal, and refresh behavior. Splitting
that too early risks duplicated state and coordination bugs.

### Generated FFI Types Everywhere

Rejected.

Generated DTOs are useful at the boundary, but using `FfiReadingRow` throughout
the UI leaks implementation details and makes later binding changes harder.
Feature code should speak app language such as `ReadingRow`.

### Heavy Redux/TCA Dependency

Deferred.

A reducer/store model could be useful later for testing optimistic behavior.
For now, the existing central `AppState` plus concern-specific extension files is
sufficient and lower ceremony.

## Guidance For AI Agents

- Read this ADR and the monorepo root's `UBIQUITOUS_LANGUAGE.md` before proposing
  large `macos` architecture changes.
- Keep domain behavior in `core`.
- Keep Swift feature code thin and presentation-focused.
- Prefer app-facing names such as `ReadingRow` over generated FFI names in
  views; `Ffi*` types must not appear outside `Bridge/`.
- Put macOS system integration in `Platform`, not in feature views.
- Put Rust bridge and FFI mapping in `Bridge`.
- Put cross-feature coordination in `State/AppState`.
- Avoid broad refactors that mix file moves with behavior changes.
- When moving files, preserve behavior first, then make naming improvements in
  separate commits when practical.
- Render saved article content natively. The optional Original view may use WebKit to load the
  HTTP(S) origin after the user explicitly selects it.
- Do not create a manual Lists model; use Tags and smart views.
