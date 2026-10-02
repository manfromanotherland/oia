# AGENTS.md — Óia

> Guidance for humans and AI agents working in this repository.
> This file is the project's north star: it explains **what we are building, why, and the
> principles that constrain how.** When in doubt, optimize for the principles below over any
> single feature.

## What this is

**Óia** is a local-first, single-user visual inspiration library: save it now, keep it
forever, and return to it when it sparks something.

You save a web page, a right-clicked image or video, or selected text from your browser. Each save
becomes a **Markdown file plus local assets on your own disk**. A native app lets you browse the
result as a mixed visual board, inspect media and quotes, open articles, search, tag, and delete
cards. There are no accounts, no servers, no logins, no telemetry. The files remain usable without
this app.

## Core principles

These are load-bearing. Most architectural questions resolve by appealing to one of them.

1. **Local-first & offline.** Everything works with no network and no backend. There is no
   server to run and nothing to log into.

2. **Files are the source of truth.** Each reading is a Markdown file with YAML frontmatter.
   Everything that matters — content, source URL, tags, and save date — lives
   *in the file*. If every other part of this project vanished, the user's library would still
   be complete and usable in any text editor.

3. **Sync is the user's choice, and external to us.** We never sync for the user. The user
   points the app at one **library folder** and syncs that folder however they like — Dropbox,
   iCloud Drive, Google Drive, git, a USB stick. The system must therefore behave correctly
   when an external process adds, removes, or modifies files at any time, possibly from another
   device. **We are never the only writer.**

4. **The database is a disposable cache.** A local index makes listing and search fast, but it
   is *derived* from the files and can be rebuilt from scratch by re-scanning the library.
   - Never store anything in the DB that cannot be recovered from the files.
   - **Never put the DB inside the synced library folder, and never sync it.** It lives
     per-device (e.g. `~/Library/Application Support/Cuttings/`).
   - Store **relative** paths (from the library root), never absolute paths — they differ per
     device.

5. **Thin clients, one shared core.** All real logic lives in the Rust engine. Native UIs are
   thin layers over it. This keeps behavior identical everywhere and makes new platforms cheap.
   Logic is **not** duplicated in Swift or JavaScript.

## Features (the product)

- **Save from the browser** — browser extension. Capture a cleaned article, right-clicked image or
  video, or selected-text quote and save it locally with its origin.
- **Save in the app** — macOS client. Drop or paste an HTTP(S) link, plain text, or image anywhere
  on the board. Text and image bytes are stored locally; a recognized public source is resolved by
  the Rust URL-save facade, while any other link stays lightweight until a later browser capture
  upgrades the same URL-derived reading.
- **Visual card board** — native app. Browse articles, images, videos, and quotes together in one
  full-width masonry layout with kind and tag filters.
- **Search** — native app. Full-text search over readings (title, content, Tags) via SQLite
  FTS5, with local visual search facts. Word-occurrence lookup and word meanings are future ideas.
- **Tags** — native app. User and automatically inferred subject Tags appear together as neutral
  Liquid Glass pills, with `tag` on user Tags and `sparkles` on machine Tags. The icon becomes a
  remove control on hover and requires confirmation; removal affects one reading, not the whole
  library. An inline field below Tags offers matching library Tags to fill the input; Return adds
  the entered name as a user Tag. Adding a machine Tag's name gives the user ownership. Both types
  are stored in each reading's frontmatter; user Tags win case-insensitive duplicates. A removed
  Tag stays excluded from later machine analysis. (The macOS mockup's "Lists" section is
  implemented as **Tags** — manual Lists are not planned.)
- **Curation** — native app. Organize cards with tags or permanently delete cards that no longer
  belong.
- **Card kind** — every reading is an **article**, **image**, **video**, or **quote**. Older files
  without a kind remain articles.
- **Origin** — web captures retain the originating page URL, canonical URL, page title/site, and
  save date. Image/video `media_url` is additional and never replaces the page origin. Source-less
  paste/drop saves use an internal `cuttings://local/...` identity instead of inventing or leaking
  a machine-local path.
- **Appearance** — native app. Light/Dark/System theme and adjustable reader typography
  (font, size, width, line height), stored as per-device preferences (not synced).

> The macOS UI is specified in [DESIGN.md](./DESIGN.md). Paste and drop are whole-board save
> gestures, not a modal "Add Link" form. Full page extraction still belongs to the browser
> extension because it has the live DOM.

## Decisions already made

- Markdown + YAML frontmatter as the storage format; files are the source of truth.
- Legacy `favorite` metadata and `note.md` sidecars remain readable and preserved for format
  compatibility, but the current macOS app neither displays nor mutates them.
- HTML cleanup runs in the extension (it has the live DOM).
- All logic in a Rust core crate; native UIs are thin and share it.
- The index is SQLite + FTS5, rebuildable, per-device, never synced.
- Use a deterministic **content-addressed id** as the reading-folder name and frontmatter id; it
  doubles as the O(1) dedup key. Articles hash the normalized origin URL. Media hash kind + origin
  + media identity; quotes hash origin + normalized selected Markdown. `canonical_url` is origin
  metadata, not a substitute identity key.
- Start native clients with macOS / Swift (SwiftUI) via UniFFI.
- **Search uses FTS5 over readings** (title, content, effective Tags) with derived visual terms
  in the disposable local index. Keep the schema open to later word-occurrence lookup and vector
  search without treating either as current scope.
- **The extension saves via a native messaging host** (thin wrapper over `core`), not
  the Downloads API.
- **Images are captured by the extension and written into the library** in each reading's own
  `assets/` folder (`articles/<prefix>/<id>/assets/`) with relative `assets/<file>` links, so saved
  readings stay readable offline and survive the source going away. The
  extension fetches each image (reusing the browser's cache) and sends the bytes; the host writes
  them without re-fetching ordinary article assets. Strictly recognized URL-only sources use the
  separate Rust source-adapter path below.
- **Standalone media and quotes are first-class saves.** Articles retain their URL-derived id.
  Image/video ids derive from kind + origin page + media identity; quote ids derive from origin
  page + selected Markdown. This lets several cards coexist from one page while exact re-saves
  deduplicate deterministically.
- **Every successful browser video save contains a local movie asset.** The extension streams a
  readable HTTP(S), `data:`, or document-scoped `blob:` source; when bytes cannot be fetched, it
  records one loop of the exact rendered element as an explicitly compatible H.264 MP4. Every
  source uses native-messaging protocol v4 with acknowledged chunks of at most 256 KiB decoded
  bytes. The core caps the complete video at 1 GiB, hashes it into a local `cuttings-asset:` file,
  and removes incomplete staging on abort, disconnect, or error. A poster-only capture is never a
  successful video save, and ordinary save messages never contain video bytes. This browser-video
  path does not fetch media in the host; recognized source adapters are a separate bounded path.
- **The macOS home is a search-first, sidebar-free masonry board.** Articles still use the
  existing native Markdown reader in the card detail overlay; no WebView is introduced.
- **The organizing model is Tags**, not manual Lists, favorites, read/unread queues, ratings, or an
  archive. The main board includes every saved item. Legacy `read_at`, `archived`, `favorite`, and
  `rating` fields remain readable as format-v1 compatibility data but are not exposed by the
  current macOS product.
- **Machine Tags are file-backed curation.** The macOS app automatically analyses saved or newly
  scanned readings while it runs: Vision supplies image subjects on the macOS 15 baseline; the
  on-device Foundation Models adapter supplies text topics on macOS 26+ when available. Rust
  persists `machine_tags` per source and `excluded_machine_tags` in Markdown, revalidates the
  source fingerprint before writing, and then refreshes the index. Raw visual terms, confidences,
  colours, and Spotlight data remain disposable per-device search facts.
- **UI preferences** (theme, reader font/size/width/line height) are per-device app
  preferences — not stored in the library and not synced.
- **URL-only saves share one Rust facade.** Strictly recognized public sources may use bounded,
  provider-specific retrieval to become complete local articles. Every other URL stays a marked
  lightweight link at the normal URL-derived id; a later full browser capture upgrades that card
  in place while preserving the user's state. See [DESIGN.md](./DESIGN.md).
- **Name:** the product name is **Óia** and the internal slug is **oia**. Use **Óia** in product copy and **Oia/oia** in source, package, and build names.
  Keep existing bundle, native-messaging, storage, and library-format identifiers stable; see
  [docs/branding.md](./docs/branding.md).
- **License / openness:** the project is **open source, multi-licensed by component**. The
  **browser extension, engine (`core`), and native
  host are MIT** — as permissive as possible to drive adoption and let anyone embed them. The
  **macOS client is GPL-3.0-or-later** — public, but anyone distributing a modified client must
  share their changes. MIT is GPL-compatible, so the GPL client can embed the MIT engine while
  the engine stays independently MIT. Add matching `SPDX-License-Identifier` headers per
  component.

## Git commit message standards

**Commit as you go.** After each coherent change, run the relevant checks and commit the
completed work without waiting for a separate request. Before handing back a task, commit all
completed changes belonging to it unless the user explicitly asks to leave them uncommitted.
Keep commits small and focused: one logical change per commit, with its directly related tests
and documentation. Separate unrelated fixes, refactors, and formatting; keep cross-component
contract changes atomic. Each commit should be independently understandable and leave the
project in a working state. Stage only task-related changes and report any checks that could
not run.

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
type(scope): short description

Optional body explaining why, not what.
```

**Types:** `feat`, `fix`, `chore`, `docs`, `test`, `refactor`, `style`, `ci`, `build`, `perf`, `revert`.

**Scopes** are optional and must be **semantic** (lowercase, human-readable): `core`, `list`, `search`, `reader`, `shortcuts`, `sidebar`, `sort`, `selection`, `macos`, `clippy`, etc. — whatever describes the area of the code.

**Never put ticket or issue numbers in a commit message** — not in the subject, not in the body. No `feat(EXT-7):`, no `CORE-14: ...` section headers in the body, nothing. If a commit covers multiple areas, name the areas in the subject or body as plain prose.

**Never add `Co-Authored-By` trailers** referencing AI assistants (Claude, GitHub Copilot, etc.). Commit messages should read as first-person author voice.

Good examples:
```
feat: add inline tag suggestions to the inspector
fix(search): include every saved item in search results
feat(core): add per-reading text highlights
test: isolate native-host library resolution from the host machine
```

Bad examples:
```
feat(EXT-7): options page with host status          ← ticket ref in scope
feat(CORE-14): tests covering the full core stack ← ticket ref in scope
CORE-6: scan_library() walks articles/...           ← ticket ref as body header
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>  ← AI co-author trailer
```

## Conventions for agents working here

- **Use the project's ubiquitous language.** Match the terms in
  [UBIQUITOUS_LANGUAGE.md](./UBIQUITOUS_LANGUAGE.md) in docs, code, UI, and commit messages.
- **Reference docs:** [ARCHITECTURE.md](./ARCHITECTURE.md) for the components, data flow, and data
  model; [DESIGN.md](./DESIGN.md) for the macOS UI/UX design.
- **Releasing:** a public release is the signed macOS `.dmg` on GitHub plus a Sparkle appcast
  entry. Follow [RELEASE.md](./RELEASE.md) for the runbook and record changes in
  [CHANGELOG.md](./CHANGELOG.md).
- Treat the **library format as a public contract** — version it; don't break readers/writers
  silently.
- Never write logic into Swift/JS that belongs in `core`.
- **The macOS reader is native SwiftUI — never use a WebView** (`WKWebView`/`WebKit`).
- **One macOS debug build:** `/Applications/Óia.app` points to
  `macos/build/Build/Products/Debug/Óia.app`. Run Xcode builds and tests from
  `macos/` with `-derivedDataPath build` so they use that single app output.
- **Always build after macOS code changes.** Before reporting a macOS fix complete,
  build the current source into that debug app and report the build result. Do not launch or
  restart Óia for routine small styling, copy, or UI edits unless the user asks. For complex
  behavior changes, launch or restart when verification requires it. A running app loads a new
  executable only after a restart.
- Never assume single-writer access to the library; always reconcile against the files.
- Never persist anything important only in the DB, and never sync the DB.
- **Native UIs update optimistically; persistence happens in the background.** A mutation already
  runs off the main thread (the core call writes the file → syncs the index), but the UI must not
  *wait* on it. Pattern: patch the in-memory published state immediately so the change shows on the
  next frame, then `await` the core call and a refresh that reconciles against the index. The
  refresh is the self-heal — a failed write re-reads as the prior truth, so optimistic guesses can
  never get stuck wrong. Don't add manual rollback paths; let the refresh be authoritative.
- **When an action removes the selected row from the current filter, advance in one motion.** If
  an optimistic edit pushes a row out of the active tag, remove it **and** move
  selection to an adjacent row in the *same* render tick. Flipping the control in place and then
  letting the row jump on the later refresh reads as a two-stage stutter. Membership ordering
  still settles on the refresh.
- Keep everything offline-capable; no network calls are required for core features.
- This is a **monorepo**: `core/`, `extension/`, and `macos/` share one Git history. The library
  format and native-messaging protocol are cross-component contracts; update every affected
  component in one atomic commit when either contract changes.
- Run Git commands from the repository root. Use semantic scopes such as `core`, `extension`, or
  `macos` when a commit is component-specific, and stage only the paths relevant to that change.
