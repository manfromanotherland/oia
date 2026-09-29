# Library Format Specification

**Version:** `1`  
**Status:** implemented — the contract the shipped components (`core`, `extension`, `macos`) conform to.

This document is the **shared contract** between all Óia components. Every component that
reads or writes library files must conform to it. Treat breaking changes as a major version bump;
land them across all affected components in the same monorepo commit.

---

## Folder layout

```
<library-root>/
  .cuttings-locks/            # persistent operational advisory-lock sidecars
    <prefix>/                 # first 2 chars of SHA-256(reading id)
      <sha256-id>.lock        # empty; one stable lock inode per reading id
  .cuttings-imports/          # persistent staging directory for streamed video imports
    video-<ulid>.tmp          # transient; removed after success, duplicate, or failure
  inbox/                     # queued files/captures; removed only after verified import
  articles/
    <prefix>/                 # first 2 chars of the id — a fan-out bucket
      <id>/                   # one self-contained folder per reading
        article.md            # the reading (Markdown + YAML frontmatter)
        assets/
          <sha256>.<ext>      # captured/imported media, linked as assets/<file>
        highlights.md         # optional — the reading's saved highlights (§ Highlights)
        note.md               # optional legacy sidecar — preserved when present (§ Legacy note sidecar)
        original.html         # optional — raw HTML snapshot for future re-processing
```

- `<library-root>` is the folder the user chooses (Dropbox, iCloud Drive, Google Drive, etc.).
- Each reading is one folder named by its id (see § ID scheme), under a two-character fan-out
  bucket so no directory grows unbounded. Everything for the reading lives inside it, so moving or
  deleting a reading is a single folder operation.
- The per-device SQLite index lives **outside** this folder (e.g.
  `~/Library/Application Support/Cuttings/`) and is **never synced**.
- Paths stored in the database must be **relative to the library root** — never absolute.
- `inbox/` is an optional capture handoff, not canonical reading data. Its independently
  versioned [capture transport](inbox-format.md) feeds the same Rust reading importer.
  Failed or incomplete inputs remain available; the index never treats them as readings.
- `.cuttings-locks/` contains empty advisory-lock sidecars used to serialize Óia writers that
  share a library on one machine. They live outside reading folders so deleting a reading cannot
  replace its lock inode while another process is waiting. Sidecars deliberately persist after an
  operation or deletion, are not reading data, and are ignored by the scanner; syncing their empty
  files does not carry a live OS lock to another device.
- `.cuttings-imports/` remains present so concurrent importers never race directory removal. Its
  uniquely named temporary files exist only while a local or browser-saved video is
  streamed and hashed, are ignored by the scanner, and are cleaned after success, duplicate,
  abort, disconnect, or error.

---

## Article file (`articles/<prefix>/<id>/article.md`)

Each saved reading is a single UTF-8 Markdown file named `article.md` inside the reading's folder,
with YAML frontmatter.

### Frontmatter schema

```yaml
---
format_version: 1                          # integer — bumped on breaking schema changes
id: 1146c9a93631d1991af3252dbc49ecd8043ab354a4386e397d555d1ca21a7199  # content-addressed (see § ID scheme) — also the reading-folder name
url: https://example.com/post/slug         # original URL as visited
canonical_url: https://example.com/post/slug  # the page's own canonical URL when known (see § URL normalization)
title: The Title of the Article            # required; extracted from page or og:title
kind: article                              # article | image | video | quote; missing means article
lightweight: true                          # optional; link save awaiting full article capture
media_url: https://cdn.example.com/image.jpg  # optional media identity; never the origin
preview_asset: assets/3f4a1b8e....jpg      # optional local card preview written by the host
favicon_asset: assets/91d0c4ab....ico      # optional locally captured page favicon
author: Jane Doe                           # optional; extracted byline
site: Example                              # optional source-site label or hostname
theme_color: "#123456"                     # optional normalized website colour for card presentation
source_profile:                            # optional versioned metadata for a recognized source
  version: 1
  source_type: social_post
  provider: x
  source_id: "2102505743278829840"
  author_handle: benspringwater
  published_at: 2026-09-22T10:00:00.000Z
  avatar_asset: assets/91d0c4ab....jpg
  attachments:
    - kind: video
      asset: assets/5ad8c2ef....mp4
      poster_asset: assets/7b14f991....jpg
      content_type: video/mp4
      width: 1920
      height: 1080
      alt: Optional source-provided description
saved_at: 2026-06-13T15:00:00Z            # ISO-8601 UTC; set once at save time; never updated
read_at: 2026-06-14T09:00:00Z             # optional legacy state; preserved for compatibility
archived: false                            # required legacy state; current macOS app ignores it
favorite: false                            # required legacy state; current macOS app ignores it
rating: 0                                  # required legacy 0–5 value; current macOS app ignores it
tags: [rust, local-first]                  # string[]; elements are lowercase, no spaces
excerpt: One-sentence summary.             # optional; shown in the list view
word_count: 1234                           # integer; word count of the cleaned body
lang: en                                   # BCP-47 language tag; optional
source_hash: sha256:abc123...              # sha256 of the cleaned Markdown body (hex); for change detection
---
```

#### Required fields
`format_version`, `id`, `url`, `canonical_url`, `title`, `saved_at`, `archived`, `favorite`,
`rating`, `tags`, `source_hash`.

#### Optional fields
`kind`, `lightweight`, `media_url`, `preview_asset`, `favicon_asset`, `author`, `site`,
`theme_color`, `source_profile`, `read_at`, `excerpt`, `word_count`, `lang`.

`kind` is written for every new card but remains optional in the parser for backwards compatibility;
an older file without it is an `article`. `lightweight` is omitted/false for ordinary captures and
is true for a link saved without a cleaned article body, from either the app or browser toolbar.

#### Rules
- `saved_at` is set once at save time and **never updated**, even when metadata is edited.
- For a web save, `url`, `canonical_url`, `title`, and `site` describe the **origin page**. A media
  card's asset identity belongs in `media_url`; it never replaces the origin. A source-less local
  text/image/video save uses a deterministic `cuttings://local/...` URL in the two required URL fields,
  leaves `site` unset, and is presented as saved locally rather than as an openable web source.
- `lightweight: true` means no cleaned article body was captured, so the body is a link. A browser
  toolbar save may still include title, canonical/source metadata, a local social preview, and a
  local favicon. A later full browser capture at the same URL replaces the captured metadata/body
  and clears this marker while preserving `saved_at`, state, rating, and tags.
- `kind` is one of `article`, `image`, `video`, or `quote`.
- `media_url` is meaningful only for `image` and `video` cards. Every newly browser-saved video is
  streamed or recorded into the library and uses `cuttings-asset:assets/<filename>` as its
  content-derived identity; transient `blob:` and `data:` URLs are never persisted. Source-less
  videos and offline migrations use the same local reference without persisting an original
  machine path. A web origin remains in `url`; legacy direct HTTP(S) video references remain
  readable for format compatibility.
- `preview_asset`, when present, must be the safe single-file shape `assets/<filename>`. It is
  derived only after captured image/poster bytes are written locally; it is never an HTTP URL.
- `favicon_asset`, when present, follows the same safe local path rule. It records a captured page
  icon separately from the full-size card preview and is never inserted into article Markdown.
- `theme_color`, when present, is optional card-presentation metadata derived from the origin
  website's declared theme colour. Its stored form is lowercase sRGB `#rrggbb`; it does not affect
  reading identity or content. Save/import adapters may supply common CSS colour forms, which the
  core normalizes centrally; unsupported values are omitted rather than failing the save.
- `source_profile`, when present, is additive provider-neutral metadata for a recognized source.
  `version`, `source_type`, `provider`, `source_id`, and `author_handle` are required inside the
  profile. `published_at` and `avatar_asset` are optional. `attachments` retains source order; each
  entry has an open string `kind`, a required safe local `asset`, and optional `poster_asset`,
  `content_type`, positive `width`/`height`, and `alt`. Provider and attachment discriminators are
  deliberately open strings so older readers can preserve future values. A social post remains an
  `article`; this profile changes its presentation, not its primary kind, identity, or tags. The
  macOS board places `social_post` readings under Links even though their text and assets are
  captured locally. Longform X Articles without that profile remain under Articles.
- `read_at`, `archived`, `favorite`, and `rating` remain part of the format-v1 compatibility
  contract. Older clients may still interpret and mutate them, so current readers preserve them
  when rewriting a file. The current macOS product does not expose them as curation controls.
- `tags` elements must be lowercase, trimmed, and contain no spaces (use `-` as separator).
- `source_hash` is recomputed on any edit to the body; the DB uses it to detect stale index entries.

### Body

The article body follows immediately after the closing `---` of the frontmatter, separated by a
blank line. It is **Markdown** (CommonMark), cleaned of navigation, ads, banners, and popups.

- The body carries **no top-level `#` heading**: the frontmatter `title` is the reading's single
  title, which the reader renders as the sole h1. The extension demotes any `#` the source used to
  `##`, so body headings start at `##`.
- Image references use **relative paths** into the reading's own `assets/` folder: `assets/<file>`
  (the article file and its `assets/` folder are siblings), e.g. `![alt](assets/3f4a1b.jpg)`.
- Do not embed images as base64.
- An **image** body is a Markdown image whose captured source is rewritten to the local asset.
- A **video** body links to its copied `assets/<file>` movie and may also reference a local poster.
  Legacy readings may still link to a durable remote media URL.
- A **quote** body contains the selected text as Markdown block quotes. Its `excerpt` may carry a
  bounded preview for the board, but the body remains the full selection.
- A **lightweight article** body is one Markdown link to its HTTP(S) URL. It is intentionally
  distinguishable from a full extension capture and may later be upgraded in place.
- A **social-post article** body starts with the post text and may append local attachment links
  after an internal `<!-- oia:attachments -->` marker. The `source_profile.attachments` array is
  authoritative for ordered native presentation; the Markdown references keep the file useful in
  ordinary text editors.

---

## Complete example

```
articles/11/1146c9a93631d1991af3252dbc49ecd8043ab354a4386e397d555d1ca21a7199/article.md
```

```markdown
---
format_version: 1
id: 1146c9a93631d1991af3252dbc49ecd8043ab354a4386e397d555d1ca21a7199
url: https://blog.example.com/posts/local-first?utm_source=hn
canonical_url: https://blog.example.com/posts/local-first
title: Local-First Software
kind: article
author: Martin Kleppmann
site: blog.example.com
theme_color: "#123456"
saved_at: 2026-06-13T15:00:00Z
archived: false
favorite: false
rating: 0
tags: [local-first, distributed-systems]
excerpt: An argument for software that works offline and gives users ownership of their data.
word_count: 3812
lang: en
source_hash: sha256:e3b0c44298fc1c149afb4c8996fb92427ae41e4649b934ca495991b7852b855
---

An argument for software that works offline and gives users ownership of their data.

## Ownership

Paragraph text…

![Diagram](assets/3f4a1b.jpg)

More content…
```

---

## Asset files (`articles/<prefix>/<id>/assets/<sha256>.<ext>`)

- Each reading's captured images and copied videos live in an `assets/` sub-folder inside the
  reading's own folder, beside `article.md`, and are linked from the body as `assets/<file>`.
- Captured social previews and favicons use the same local asset store. Frontmatter addresses them
  only through the relative `preview_asset: assets/<file>` and `favicon_asset: assets/<file>` roles;
  their remote source URLs are not the stored presentation references.
- Filename is the **lowercase hex SHA-256** of the file's raw bytes, with an extension chosen from
  its `Content-Type` (falling back to the source URL for images): e.g. `3f4a1b8e....jpg`.
- Article and standalone images are normally **captured by the browser extension** (from the page's cache
  where possible) and sent to the host. Every browser video is streamed in acknowledged chunks;
  when source bytes cannot be fetched, the extension records the exact rendered element as a
  compatible H.264 MP4. Videos are never embedded in an ordinary save message. Standalone images
  and videos may also arrive from the app's paste/drop path. The My Mind migration adapter may
  fetch link metadata, social previews, and favicons during a write unless `--offline` is selected.
  A strictly matched source adapter may instead retrieve bounded public metadata/media through the
  explicit Rust URL-save facade, stage it locally, and hand it to the same core writer. Other write
  paths do not fetch. An image the extension couldn't capture is left as a remote URL in the Markdown and is
  never re-fetched; the reader shows a labelled placeholder for it.
- The original HTML snapshot is optional. If kept, it lives as `original.html` inside the reading's
  folder for future re-processing.

---

## Highlights (`articles/<prefix>/<id>/highlights.md`)

A reading's saved highlights live in `highlights.md` inside the reading's folder — one file per
reading, absent when the reading has none. Each highlight is the verbatim selected text as a
Markdown block quote, ended by an HTML comment carrying a stable id:

```markdown
> The exact text the user highlighted.
<!-- hl 01J9Z8X7Q2VBKN3P4HXYZ01AB -->
```

The scanner keys on the fixed `article.md` name, so a reading's `highlights.md` (and its `assets/`)
are never mistaken for readings.

---

## Legacy note sidecar (`articles/<prefix>/<id>/note.md`)

Older clients and importers may have written one plain UTF-8 CommonMark `note.md` beside a
reading's article file and highlights. The current macOS app does not display, create, edit, or
remove this sidecar. Routine scans, article rewrites, metadata edits, and capture upgrades preserve
it as written. Deleting the reading still deletes its whole reading folder, including any sidecars.

The legacy note remains separate from the captured body and its `source_hash`. It is not mirrored
in the disposable index or included in full-text search. As with `highlights.md`, the scanner's
fixed `article.md` entry point ensures a note is never mistaken for another reading.

---

## ID scheme

A **reading id** is a deterministic lowercase-hex SHA-256 content address. The identity input
depends on the card kind:

- **Article:** normalized origin `url` (the existing `url_id` behavior).
- **Web image/video:** kind + normalized origin `url` + `media_url` identity. Images and legacy
  video readings may use a durable direct URL; newly copied browser videos use their
  content-derived `cuttings-asset:` reference.
- **Quote:** normalized origin `url` + normalized selected Markdown body.
- **Source-less text:** a `cuttings://local/quote/<hash>` origin derived from whitespace-normalized
  text, then the ordinary quote identity rule. Whitespace-only variations deduplicate.
- **Source-less image:** a `cuttings://local/image/<hash>` origin/media identity derived from the
  validated imported image bytes, then the ordinary image identity rule. File names do not affect
  identity.
- **Source-less video:** a `cuttings://local/video/<hash>` origin derived while streaming a copy of
  the selected file into the library. Its `cuttings-asset:` media reference points at that copy;
  the original path and file name do not affect identity.
- **Browser video:** normalized HTTP(S) origin `url` plus a content-derived `cuttings-asset:` media
  reference. The source media URL never participates in identity; identical bytes from different
  origins remain distinct readings.
- **Migrated web image/video:** normalized origin `url` plus a content-derived `cuttings-asset:`
  media reference for the copied bytes. The source export path never participates in identity or
  persists in the library; identical bytes from different origins remain distinct readings.

This permits several media/quote cards from one origin while an exact repeat save deduplicates.

- 64 hex characters, e.g. `1146c9a93631d1991af3252dbc49ecd8043ab354a4386e397d555d1ca21a7199`.
- **Deterministic** — the same kind-specific identity input yields the same id, so the id doubles as
  the dedup key. The host hashes the input and stats the one folder it would occupy, with no scan or
  index lookup.
- The id is the **reading-folder name** (under its `<prefix>` bucket) and the frontmatter `id`
  field. They must match — a folder whose name disagrees with its `article.md`'s `id` is ignored.
- Not time-sortable: the reading list orders by `saved_at` via the index, not by id.

Highlight ids (the `<!-- hl ... -->` markers) are **ULIDs** — 26-character Crockford Base32,
sortable by creation time — because a highlight is identified by when it was made, not by content.

---

## URL normalization & identity

A reading's identity is the **normalized visited URL**: the host normalizes the `url` and the
reading id is its SHA-256 (§ ID scheme). Apply these rules in order:

1. Lowercase the scheme and host.
2. Strip a leading `www.` from the host.
3. Remove the default port (`:80` for http, `:443` for https).
4. Strip the fragment (`#...`).
5. Strip tracking query parameters: `utm_*` (prefix), `fbclid`, `gclid`, `mc_cid`, `mc_eid`, `ref`,
   `source`, `campaign` (exact match).
6. Sort the remaining query parameters — they may be meaningful (pagination, article ids), so keep
   them, but sort so their order can't produce two ids for one page.
7. Remove a trailing `/` from the path **unless** the path is just `/`.

The original origin-page `url` (pre-normalization) is preserved for browser captures.
`canonical_url` stores the page's own `<link rel="canonical">`/`og:url` when known, for reference —
it is not substituted with a CDN or direct media address. The URL-only app path normalizes the URL
before writing because no page metadata exists yet. Source-less local identities are already stable
internal URLs and do not go through web URL normalization. Two different normalized web origins for
the same content can still produce two readings — an accepted trade-off of origin-aware capture.

A recognized source adapter may canonicalize equivalent provider aliases before the first save (for
example, X and Twitter host aliases for one post). If a lightweight placeholder already exists for
the submitted URL, the full source capture upgrades that same URL-derived reading instead. Later
alias checks and saves use `source_profile.provider` plus `source_profile.source_id` to find the
existing reading without changing its durable id.

---

## Current macOS board scopes

The current macOS app has no sidebar and does not expose note, favorite, read, archive, or rating
workflows:

| Scope | Filter |
|-------|--------|
| **All** | every saved item, regardless of legacy state fields |
| **Images** | image readings |
| **Videos** | video readings |
| **Articles** | captured articles other than social posts, including longform X Articles |
| **Links** | lightweight URL-only article placeholders and captured social posts |
| **Quotes** | quote readings |

Article and Link item-type search terms use the same distinction as these board scopes. A captured
social post keeps `kind: article` and `lightweight: false` in the library file; its Links placement
is a presentation rule, not a file-format conversion.

The core continues to parse, index, and round-trip `read_at`, `archived`, `favorite`, and `rating`
so opening an existing library does not strip data used by an older client. Those fields no longer
hide cards from **All**, and existing `note.md` sidecars remain untouched.

---

## Format versioning

- `format_version` starts at `1`.
- **Additive changes** (new optional frontmatter fields, new asset conventions) are backwards
  compatible — do not bump the version.
- **Breaking changes** (renamed/removed required fields, changed semantics) bump the integer.
- Readers must reject files with a `format_version` higher than the version they support, rather
  than silently misread them.
- All three components (`core`, `extension`, `macos`) must be updated together on a version bump.

---

## What lives in the library vs. outside it

| Belongs in library (synced) | Belongs outside library (per-device, never synced) |
|-----------------------------|----------------------------------------------------|
| `articles/<prefix>/<id>/article.md` | SQLite index (`~/Library/Application Support/Cuttings/`) |
| `articles/<prefix>/<id>/assets/*` | App preferences (theme, font, library path) |
| `articles/<prefix>/<id>/highlights.md` | Native messaging host manifest |
| `articles/<prefix>/<id>/note.md` (optional legacy sidecar) | |
| `articles/<prefix>/<id>/original.html` (optional) | |
| `.cuttings-locks/<prefix>/<sha256-id>.lock` (operational, empty) | |
