# Architecture

Óia is three apps that share one data format and one Rust core: a browser **extension**
captures cleaned articles, standalone media, and selected-text quotes; a **native messaging host**
writes them into a plain-file library; and the **macOS app** (embedding the core) both accepts
paste/drop saves and shows the library as a visual card board. Files are the source of truth; the
index is a disposable, per-device cache.

## How it works

```
 ┌─────────────────┐   cleaned MD + assets    ┌──────────────────────────┐
 │ Browser ext.    │ ───────────────────────▶ │ Native messaging host    │
 │ (capture cards) │   (native messaging)     │ (wraps core)             │
 └─────────────────┘                          └───────────┬──────────────┘
                                                           │ writes files only
                                                           ▼
                                              ┌──────────────────────────┐
                                              │   Library folder (disk)  │  ◀── user syncs this
                                              │   articles/ assets/ ...  │      (Dropbox/iCloud…)
                                              └───────────┬──────────────┘
                                         write + watch +  │  reconcile on launch
                                                          ▼
 ┌─────────────────┐   UniFFI saves/queries   ┌──────────────────────────┐
 │  macOS app      │ ◀──────────────────────▶ │  core (Rust)             │
 │ paste/drop + UI │                          │ save • index • search   │
 └─────────────────┘                          └───────────┬──────────────┘
                                                          ▼
                                              SQLite + FTS5 (per-device, NOT synced)
```

The extension toolbar can extract and clean the current article, save a lightweight link, or capture
the full scrollable page as one long screenshot. The context menu also captures a right-clicked
image, saves a right-clicked video as a local movie, or turns selected text into a quote. Ordinary
captures hand Markdown, metadata, and image bytes to a small native host; every video uses the
bounded stream below. The host writes both paths into the library folder.
Article and link saves retain live Open Graph/Twitter metadata plus local social-preview and favicon
assets without injecting those head assets into the cleaned Markdown body. When a website declares
a usable theme colour, the core stores it as normalized `theme_color` card-presentation metadata.
The macOS app also accepts dropped or pasted HTTP(S) links, text, and images. Both native entry
points call the same core save service: local bytes are copied into the library and source-less
items receive a private deterministic identity. URL-only saves first pass through the Rust URL-save
facade. A recognized public source is resolved into a complete local article by its bounded source
adapter; an unknown URL remains lightweight so a later full browser capture can upgrade it. The app
watches the folder and indexes every new file for the masonry board, full-text search, type filters,
and tags — so browser saves, in-app saves, and files delivered by sync reconcile through the same
index path. Captured social posts keep their complete local article files and assets but appear under
Links on the macOS board and in Link item-type search. Longform X Articles remain under Articles.

After a save or scan finds a new or changed reading, the macOS app analyses its saved content while
the app is running. Vision classifies local image assets on the macOS 15 baseline; on macOS 26 and
later, the on-device Foundation Models content-tagging adapter can infer text topics when its model
is available. The Swift adapters return platform observations; the Rust core owns Tag selection,
case-insensitive precedence, durable exclusions, file writes, and index reconciliation. Analysis
completion rechecks the source fingerprint before writing, so a result for an older file cannot
replace Tags for newer content. Machine Tags are added automatically, without a review step.

The iOS **Óia!** Shortcut publishes sealed captures into `inbox/`
inside the user's synced library. The Mac requests any missing iCloud bytes and
passes ready files to the shared Rust Inbox importer. Rust validates private
snapshots, sends URL-only captures through the same URL-save facade, verifies durable results, and
removes only unchanged successful inputs. It then yields to a single index
reconcile. Inbox processing starts after the existing board is published and
runs only while the Mac app is open. See [the capture contract](docs/inbox-format.md)
and [Shortcut setup](docs/ios-shortcut.md).

Version-2 Inbox captures explicitly request one Instagram post/carousel item.
The app opts into a Rust adapter that validates the URL/index and launches the
bundled Instaloader transport script in a per-device Python environment. Only
this explicit request path performs network retrieval; the default core Inbox
entry point and ordinary file imports remain offline. The resulting private
media file passes through the same byte validation, durable import and safe
request cleanup as supplied media. No hosted service or background daemon is
introduced.

Every card kind records its origin page in `url`/`canonical_url` plus its page title/site and save
date. A browser-saved video's `media_url` is always a content-addressed local `cuttings-asset:`
reference. The extension streams readable HTTP(S), `data:`, and document-scoped `blob:` bytes; if
the selected source cannot be fetched, it records one loop of the exact rendered element. The
recording fallback requires an explicit H.264 MP4 capability and never saves an
AVPlayer-incompatible WebM. Transient `blob:` and `data:` values are never persisted. Neither a
CDN/media address nor a machine-local file path replaces the origin page.

## Components

The system is a **monorepo** with three top-level components sharing one library format and one
Rust core. Keeping them in one Git history lets protocol and format changes land atomically across
every affected component.

### Browser extension (`extension`)
- **Responsibility:** expose toolbar actions for an article, lightweight link, or full-page
  screenshot; capture a selected image; record a selected video and poster; or capture selected
  text as a quote.
  Every path produces Markdown plus origin metadata and any local image bytes needed by the card.
- **Why cleanup happens here:** the extension has the *live, rendered DOM*, so it sees JS-rendered
  content and pages the user is logged into. The engine never sees the page.
- **Stack:** Manifest V3, TypeScript (Readability-style extraction + HTML→Markdown).
- **Hard constraint:** MV3 extensions can't write files to disk, so saving goes through a **native
  messaging host** — a small native binary (a thin wrapper over `core`) that receives the cleaned
  Markdown + assets and writes them into the library.
- **Video boundary:** every successful browser video save contains a local movie asset. HTTP(S),
  `data:`, and document-scoped `blob:` sources are read in the isolated or owning page world; an
  unreadable source is captured from the exact rendered element as H.264 MP4. Bytes are relayed
  over one persistent native connection in acknowledged chunks of at most 256 KiB decoded bytes.
  The core streams at most 1 GiB into `.cuttings-imports/`, validates, hashes, and atomically
  commits the local asset, and removes incomplete staging on abort, disconnect, or error. Ordinary
  save messages carry no video bytes, and the host performs no network request.

### Engine (`core`, Rust)
- **Responsibility:** owns the library format and all logic — validate and write extension or
  URL-only saves, scan and index the library, full-text search, user and machine Tags, highlights,
  and reconcile changes that arrive via sync. The provider registry currently recognizes public X
  post URLs;
  its seam is provider-neutral so additional social and video sources can be added independently.
- **Shape:** a core library crate reused by the other native pieces (the macOS app and the native
  messaging host both link it). Not a long-running daemon.
- **Network boundary:** ordinary capture/write/index APIs perform no network requests. The explicit
  URL-save facade may call a source adapter only after a strict host-and-route match. Each adapter
  uses fixed HTTPS hosts, bounded redirects, timeouts, response-size caps, media validation, and
  staging; the writer verifies every content address and commits all required assets before making
  the article file visible. Recognized-source failure is reported rather than silently writing a
  misleading lightweight link.
- **Index:** local SQLite database with FTS5. Rebuildable; per-device; never synced.
- **Tag persistence:** `tags` contains user labels; `machine_tags` contains image/text source
  entries with `source`, `source_fingerprint`, `analyzer_version`, and inferred `tags`;
  `excluded_machine_tags` contains case-folded keys the user removed. The effective Tag set is
  a case-insensitive union with user spelling and presentation ownership taking precedence.
  Removing an effective Tag removes the user label, if present, and records its exclusion so an
  existing or future machine result cannot restore it. The core revalidates the analysed source,
  writes frontmatter first, re-reads it, then updates the index. The index caches effective Tags
  and machine-only names for presentation.

### My Mind migration adapter (`core/mymind-import`)
- **Responsibility:** map a My Mind `cards.csv` export and its local media into the shared core save
  inputs. Preview planning is offline and never changes the library.
- **Link enrichment boundary:** writes enrich HTTP(S) link rows by default by fetching page metadata,
  a bounded social preview, and a favicon. `--offline` opts out and writes URL-only lightweight
  links. The adapter passes captured metadata and bytes into the network-free core.
- **Local result:** captured social previews and favicons become relative `assets/<file>` roles in
  each reading folder. A supported website theme colour becomes normalized `#rrggbb`
  `theme_color` metadata for the card palette.
- **Existing-library cleanup:** the explicit enrichment migration snapshots opaque IDs before any
  writes. It removes confirmed 404/410 links and twice-unreachable origins, with a batch-level guard
  that retains unreachable links when failures resemble a local or network-wide outage. Reachable
  rejected, rate-limited, oversized, and non-HTML responses are retained.

### macOS client (`macos`, Swift)
- **Responsibility:** the native UI — browse a mixed masonry board, filter by card kind and tag,
  open articles, inspect images/videos/quotes, search, and save supported drop/paste payloads;
  appearance settings.
- **Stack:** Swift / SwiftUI, embedding `core` via **UniFFI**-generated bindings.
- **Native rendering only — never a WebView.** The reader renders article Markdown as a native
  SwiftUI view tree via Apple's [`swift-markdown`](https://github.com/apple/swift-markdown) parser —
  proper macOS typography, text selection, Light/Dark, and accessibility with no web engine and no
  script-execution surface. The UI is specified in [DESIGN.md](./DESIGN.md).
- Card detail is a full-window native Gallery destination: the existing Markdown reader handles
  articles and quote bodies; image/video cards use local preview assets and source/media actions.
  Gallery navigation follows the current board order, while an optional leading Inspector
  exposes the origin page when one exists and identifies source-less cards as saved locally.
  Lightweight links bypass Gallery and open their origin directly in the system browser. Captured
  social posts appear under Links but retain their native detail view and local content.
- Owns the local index and watches the library folder for changes (including files arriving via
  sync), reindexing incrementally.
- Owns narrow macOS adapters for Vision image classification and, when available, Foundation
  Models text topic extraction. Save completion and library reconciliation enqueue analysis for
  missing or stale sources while the app is running. Image classification completion and cache
  hydration write the image machine-Tag source; text inference checks the content fingerprint
  before updating the text machine-Tag source. The UI presents the effective Tags together with
  distinct user and machine colours.

### Why this shape
- One writer to the index (the app), so no SQLite contention. The host only writes files; the app
  picks them up via its folder watcher.
- The same Rust core powers both the save path and the UI — no duplicated logic.
- The index is fully rebuildable, so a fresh sync on a new device "just works" after a scan.

## Data model — the library

Each reading is a **self-contained folder** under a **library folder** the user chooses and syncs.
The folder is named by a deterministic content-addressed id under a two-character fan-out bucket so
no directory grows unbounded, and it holds everything for that reading:

```
<library-root>/
  articles/
    <prefix>/                 # first 2 chars of the id (fan-out bucket)
      <id>/                   # one folder per reading
        article.md            # Markdown body + YAML frontmatter (source of truth)
        assets/<hash>.<ext>   # captured images, linked as assets/<file> from article.md
        highlights.md         # optional — the reading's saved highlights
        note.md               # legacy sidecar — preserved when present
        original.html         # optional — raw HTML snapshot for re-processing
```

Keeping a reading in one folder makes image links trivially relative (`assets/<file>`, no `../`),
makes a reading one movable unit, and makes deletion a single guarded folder removal. Article
identity remains the normalized visited URL. Image/video identity combines the kind, normalized
origin page, and media identity. Images may use a durable media URL; every newly browser-saved
video uses its content-derived local asset reference. Quote identity combines the normalized
origin page and normalized selected Markdown. Exact repeat saves therefore deduplicate
while multiple clips from one page can coexist. Source-less pasted text and images use
content-derived local identities; their stored `cuttings://local/...` URLs are internal provenance,
never openable web sources.

Older libraries may contain a user-authored `note.md` sidecar. It stays separate from the captured
article body and source hash. The current macOS app does not display or mutate it; routine scans,
article rewrites, and metadata edits leave it untouched so existing library data is not discarded.

The card metadata is additive and backwards compatible:

- `kind`: `article`, `image`, `video`, or `quote` (missing means `article`).
- `media_url`: optional image/video identity: either a durable direct address or a content-derived
  `cuttings-asset:` reference for locally copied media. The page origin remains in `url`.
- `preview_asset`: optional safe `assets/<file>` path derived after the host writes captured image
  bytes. It drives the board thumbnail and is never a remote URL.
- `favicon_asset`: optional safe `assets/<file>` path for a captured page icon. It remains distinct
  from the full-size card preview and is never a remote URL.
- `theme_color`: optional lowercase `#rrggbb` presentation hint derived from the origin website.
  The card palette may use it as a base colour; it does not participate in reading identity.
- `source_profile`: optional versioned, provider-neutral metadata for a recognized source. A social
  post remains `kind: article`; the profile records its provider/source identity, author handle,
  publication time, avatar asset, and ordered local attachments so clients can render a source-aware
  card without inventing a new kind or tag. The macOS board places such social posts under Links;
  their full article storage format and offline assets remain intact.
- `lightweight`: optional `true` marker for a link saved without cleaned article content, from the
  app or browser toolbar. A later full browser capture
  replaces that placeholder at the same article id and clears the marker while preserving user
  state.
- `tags`: user-authored or imported labels.
- `machine_tags`: per-source (`image` or `text`) subject labels in records with `source`,
  `source_fingerprint`, `analyzer_version`, and `tags`; each source is replaced only after its
  current input is verified.
- `excluded_machine_tags`: case-folded Tag keys removed by the user, kept in the article file so
  later analysis respects the removal. Adding a matching user Tag gives it ownership in the
  effective Tag set.

- **Frontmatter is the source of truth** for metadata (title, user and machine Tags, Tag
  exclusions, and legacy format-v1 state fields, including `favorite`). The full, versioned
  schema is [`docs/library-format.md`](./docs/library-format.md); the native-messaging contract is
  [`docs/native-messaging.md`](./docs/native-messaging.md).
- **The index DB is a disposable cache** — derived from the files, rebuildable by re-scanning, and
  stored **outside** the library (per-device, never synced).
- Raw Vision labels/confidences, OCR text, colour data, and Spotlight donations are disposable
  per-device search facts. The core writes qualifying subject labels as machine Tags in frontmatter.
- **Mutations are file-first, index-second, and atomic.** Every metadata setter writes the `.md`
  frontmatter first, then syncs the derived index row from the re-read file. A folder watcher
  reconciles the index *from* the files, so writing the DB first would be clobbered on the next
  reconcile — treat each setter as one indivisible "persist" step.
