# UBIQUITOUS_LANGUAGE.md - oia

This glossary defines the shared product language for `oia`. Use these
terms in docs, code discussions, issue titles, UI architecture, and commit
messages so the project stays consistent across the extension, core, native
host, and macOS app.

## Naming Rules

- Write **Óia** as the product name and **Save to Óia** as the extension name,
  and **Óia!** as the Shortcut name. Use ASCII **Oia/oia** for code and paths. Existing compatibility
  identifiers are listed in [docs/branding.md](./docs/branding.md).
- Use **save** for the user action that adds a page, image, video, or text quote
  to the library, whether from the extension or by pasting/dropping in the app. Saving is
  time-neutral: people save things to revisit
  *and* things they already consumed and want to keep. Do not use download or
  bookmark for this action.
- Use **capture** for the extension's technical extraction step (live DOM or
  right-click context → Markdown, origin metadata, image bytes). The user saves;
  the extension captures.
- Use **reading** for the established internal saved-domain object. In user-facing copy, prefer
  **card** or **saved item**: people browse, search, revisit, tag, highlight, or delete it.
- Use **article file** only when talking about the on-disk `article.md` inside a
  reading's folder.
- Use **reading folder** for the per-reading folder `articles/<prefix>/<id>/`
  that holds a reading's article file, its assets, and its highlights.
- Use **library** or **library folder** for the user-selected folder that holds
  readings and assets.
- Use **index** for the local SQLite database. Do not call it the source of
  truth.
- Use **tag**, not list, collection, folder, or category.
- Use **extension** or **browser extension**, not plugin.
- `read_at`, `archived`, `favorite`, and `rating` are legacy format-v1 field names, not current
  product vocabulary.

## Core Domain Terms

| Term | Definition |
|------|------------|
| App | The user-facing product as a whole. In implementation, this currently means the browser extension, native messaging host, Rust core, and macOS client. |
| Óia | The product name shown to users. |
| oia | The internal project slug used for the monorepo, packages, and file paths. Not shown to users. |
| Library | The folder chosen by the user that stores their synced, durable reading data. |
| Library folder | Same as Library, used when emphasizing the on-disk directory. |
| Library root | The absolute folder path selected on one device. Data stored inside the index must still use paths relative to this root. |
| Save | The user action that adds a page, media item, or text to the library as a new reading/card, from either the extension or the app's paste/drop path. |
| Capture | The extension step that turns a page or right-click context into Markdown, universal origin metadata, optional local image bytes, or a bounded document-video stream before the save is written. Internal/technical term; users just "save". |
| Reading | One saved item in the user's library. A reading is backed by an article file plus optional assets and highlights inside its reading folder. |
| Card | User-facing visual representation of a reading on the macOS masonry board. Do not rename the internal `Reading` domain type merely to match presentation. |
| Card kind | The reading's capture/rendering kind: `article`, `image`, `video`, or `quote`. A missing kind on an older file means `article`. |
| Origin | The source page for a web card: `url`, `canonical_url`, page title/site, and save date. For image/video cards this is deliberately distinct from `media_url`. Source-less app saves instead carry a private local identity. |
| Media URL | Optional media identity for an image/video card. Images may retain a durable direct URL; every newly browser-saved video uses a content-derived `cuttings-asset:` reference to its local movie. Legacy direct video URLs remain readable. The media identity supplements the origin and never replaces the page URL. |
| Preview asset | Optional safe local `assets/<file>` reference used by the masonry card. The host derives it only after captured image/poster bytes have been written. |
| Quote | A text card whose full text is stored as Markdown. Browser selections retain their page origin; source-less paste/drop text uses a private local identity. |
| Lightweight link | An article card with no cleaned article body. It may be created by paste/drop or the browser toolbar and may retain page metadata, a social preview, and a favicon. It is explicitly marked `lightweight: true`; a later full browser capture upgrades the same reading in place. It shares the Links board scope with captured social posts but has no local post body. |
| Recognized source | A public URL whose host and route match a Rust source adapter. A URL-only save can retrieve its durable text, metadata, and supported media without needing a live browser DOM. Unknown URLs retain the ordinary lightweight-link behavior. |
| Source profile | Optional, versioned, provider-neutral frontmatter describing a recognized source and its ordered local attachments. It changes presentation without introducing a new card kind or Tag. |
| Social post | A full `article` reading with a `source_profile` whose type is `social_post`. Its text and attachments are durable local content, and its board card and detail may use a provider-aware presentation. It appears in the Links board scope and Link item-type search despite its full article storage format. |
| Local identity | A deterministic, non-web `cuttings://local/...` URL used for source-less text, image, or video saves. It prevents machine-local paths leaking into synced files and is never shown as an openable source. |
| Reading folder | The per-reading folder `articles/<prefix>/<id>/` (named by the reading id, under a two-character fan-out bucket) that holds the reading's `article.md`, its assets and highlights, and any preserved legacy sidecars. Moving or deleting a reading operates on this one folder. |
| Article file | The `article.md` file inside a reading folder (`articles/<prefix>/<id>/article.md`) that stores one reading's frontmatter and body. |
| Frontmatter | YAML metadata at the top of an article file. It is the source of truth for reading metadata and state. |
| Body | The cleaned Markdown content after frontmatter in an article file. |
| Asset | A local image or video stored in the reading's own `assets/` folder (`articles/<prefix>/<id>/assets/`) and linked from the body with a relative `assets/<file>` path. |
| Original HTML | Optional raw HTML snapshot stored as `original.html` inside the reading folder for future reprocessing. |
| Highlight | A saved selected text passage for one reading. Highlights are stored in the reading folder, separate from the article file. |
| Highlight file | The `highlights.md` file inside a reading folder (`articles/<prefix>/<id>/highlights.md`) that stores that reading's saved highlights. |
| Legacy note file | A `note.md` sidecar written by an older client or importer. The current macOS app does not display or mutate it, but preserves it when present. |
| Reading id | Deterministic lowercase-hex SHA-256 content address. Web articles hash the normalized origin URL; web image/video cards hash kind + normalized origin + media identity; web quotes hash normalized origin + normalized selected Markdown; source-less app saves derive identity from their content. It names the reading folder and frontmatter `id`. |
| Content-addressed id | An id derived from stable card identity rather than assigned, so identical input yields an identical id without a coordinator. |
| ULID | Sortable id scheme (Crockford Base32). Used for highlight ids; reading ids are content-addressed (see Reading id), not ULIDs. |
| Source URL | The originating page URL stored as `url` for a web card. For articles its normalized form is the whole identity; for media/quote it is one component of identity. A source-less app save stores a local identity in the same required field. |
| Canonical URL | The page's own canonical URL when known, stored as `canonical_url` for reference. Dedup keys on the reading id (from the normalized *visited* URL), not this field. |
| Source hash | Hash of cleaned content used to detect stale index rows and content changes. |
| Format version | Integer frontmatter version for the library file contract. |

## Curation And Organization

| Term | Definition |
|------|------------|
| Tag | A subject or curation label stored in a reading's frontmatter and indexed by search. The Tags section combines user and machine Tags; matching ignores case and the user Tag wins a duplicate. Its label searches; its icon becomes a per-reading remove control on hover, requiring confirmation. |
| User Tag | A label added by the user or retained from an import in `tags`. It has precedence over a machine Tag with the same spelling ignoring case and shows the SF Symbol `tag`. |
| Machine Tag | A subject label inferred locally from saved text or media. `machine_tags` groups these labels by `image` or `text` source with `source_fingerprint` and `analyzer_version`. It appears beside user Tags in neutral Liquid Glass styling, identified by `sparkles`. |
| Tag exclusion | A case-folded label key in `excluded_machine_tags`. Removing a Tag from one reading removes a matching user label there and records this key, so a machine result cannot restore it later. It does not delete the Tag name from other readings. |
| Board scope | Exactly one toolbar selection: All, Images, Videos, Articles, Links, or Quotes. Images and Videos each select their matching reading kind; Articles selects full articles other than social posts, including longform X Articles; Links selects lightweight article placeholders and captured social posts. |
| Board filter | The selected board scope, free-text query, and any scoped search terms, applied as one intersection to the board. |
| Search token | A native search-field pill created from an item-type or exact Tag suggestion. Tokens narrow the free-text search and board scope by intersection. |
| All | The unfiltered board scope. It includes every saved item, including files carrying a legacy `archived: true` value. |
| Legacy state field | `read_at`, `archived`, `favorite`, or `rating` in a format-v1 file. The core preserves these for compatibility; the current macOS app does not display or mutate them. |
| Board selection | The transient set of cards selected on the macOS board. A plain click or arrow move replaces it; Shift-click or Shift-arrow extends it from an anchor. Board actions such as delete apply to the complete set. |
| Focused card | The one card within the board selection that receives spatial arrow-key navigation and single-reading actions. When an optimistic edit removes it from the active board scope, focus advances to an adjacent matching reading in the same render tick. |
| Open reading | The single reading shown in Gallery detail. Double-clicking a card opens it and collapses any board multi-selection to that card. |

## Storage And Sync

| Term | Definition |
|------|------------|
| Files are the source of truth | The rule that durable reading data lives in Markdown files, not only in a database. |
| File-first mutation | A state change that writes frontmatter first, then updates the derived index from the re-read file. |
| Index | Per-device SQLite database used for fast listing and search. It is disposable and rebuildable. |
| Reconcile | Process that scans files and brings the index back in line with the library. |
| Rebuild | Full index recreation from the library files. |
| Sync | User-managed movement of library files between devices through tools outside this app, such as iCloud Drive, Dropbox, Google Drive, git, or a USB drive. |
| External writer | Any process other than the current app instance that can add, remove, or edit files in the library. |
| Conflict | A duplicate or conflicted file created by an external sync tool. Conflict detection and UX are still future work. |
| Per-device data | Data that belongs outside the library and should not sync, such as the index, UI preferences, and local app configuration. |
| Relative path | A path stored relative to the library root. The index should store relative paths because absolute paths differ per device. |

## Components

| Term | Definition |
|------|------------|
| Extension | Browser extension that saves a cleaned article, lightweight link, or one long screenshot of the full scrollable page from its toolbar, and captures a clicked image/video or selected-text quote from its context menu. It sends ordinary captures as Markdown, origin metadata, and optional image bytes; every video instead uses the acknowledged browser video import stream. |
| Browser video import | Protocol-v4 transfer used by every browser video save. The extension streams readable source bytes or records one rendered loop as compatible H.264 MP4 when the source cannot be fetched, then sends acknowledged chunks through the native host; the core commits a content-addressed local asset and cleans incomplete staging. |
| Site adapter | Extension pre-processor for a specific host (e.g. X/Twitter) that reshapes single-page-app markup before generic extraction, so content Readability would otherwise discard is preserved. |
| Source adapter | Rust resolver for one recognized public source. It classifies URLs, retrieves bounded provider metadata and media, and hands a complete staged capture to the shared writer. Provider-specific transport details do not enter the library format or clients. |
| URL save facade | The core entry point used by the Shortcut Inbox, native host, and app paste/drop path. It asks source adapters to resolve recognized URLs and otherwise writes an ordinary lightweight link. |
| Native messaging host | Native binary called by the extension. It writes readings and assets to the library through `core`. |
| Core | Rust engine that owns save/import behavior, recognized-source resolution, the library format, file parsing/writing, indexing, search, tags, highlights, legacy compatibility, and the UniFFI surface. |
| macOS client | SwiftUI app that lets the user save by paste/drop, browse, search, revisit, tag, highlight, delete, and configure the library. |
| UniFFI bindings | Generated Swift bridge that lets the macOS client call the Rust core. |
| Thin client | A client that delegates domain logic to the Rust core and keeps only presentation, navigation, and local UI state. |
| Folder watcher | macOS file-system watcher that notices library changes and triggers reconcile. |
| Native host manifest | Browser registration file that tells the browser where the native messaging host binary lives. |

## UI And Interaction

| Term | Definition |
|------|------------|
| Reader | Main article reading surface in the macOS app. It renders Markdown natively and supports local assets, text selection, highlights, and typography settings. |
| Card board | Full-width mixed masonry presentation of reading rows for the active board scope and search query. |
| Reading list | Legacy name for the old row-based macOS presentation and for the core listing API; the current user-facing home is the card board. |
| Search | A free-text query over indexed reading title, content, user and machine Tags, and derived visual terms, optionally narrowed by exact Tag or item-type tokens. Free text remains broad; scoped tokens keep their own meaning. |
| Visual search term | A disposable index value derived from local image analysis, such as a detected raw label or colour family. It can improve free-text search but is not itself a Tag or a separate suggestion, and is not written to the reading file. The core writes qualifying subject labels as machine Tags through a separate file-first write. |
| Board order | Fixed card-board ordering: newest saved first while browsing and relevance while searching. |
| Optimistic UI | UI pattern where local state changes immediately, then the core write and refresh reconcile against persisted truth. |
| Refresh | UI reload from the core/index after a mutation, sync, filter change, or search change. |
| One-motion removal | Interaction rule where a row that leaves the active filter is removed and selection advances in the same render tick. |
| Settings | macOS surface for appearance, typography, library folder, and native host status. |
| Appearance preference | Per-device Light, Dark, or System setting. |
| Typography preference | Per-device reader font and size setting. |

## Positioning And Marketing

Terms for user-facing copy: the landing page, app store text, READMEs' first
paragraphs, and the welcome article.

| Term | Definition |
|------|------------|
| Inspiration library | Preferred product category: a visual, permanent place for articles, images, videos, and quotes that spark ideas. |
| Visual library | Shorter supporting description when "inspiration library" has already established the product. |
| Local-first | Marketing shorthand for the no-accounts, no-servers, files-on-your-disk principles. |
| Save | The user-facing verb for adding a page, media item, or quote to Óia. |

## Terms To Avoid Or Use Carefully

| Avoid | Use Instead | Reason |
|-------|-------------|--------|
| List | Tag, filter, or board | Manual Lists are not a product model. |
| Database source | Index | The database is disposable and derived from files. |
| Sync engine | External sync | The app does not sync for the user. |
| Article as domain object | Reading | Article is useful for file names and reader UI, but reading is the product entity. |
| "Reading"/"readings" as a user-facing noun | Card, saved item, article, image, video, or quote | The internal domain term should not make the product sound like a reading queue. |
| Starred or favorite | Tag | The current product uses tags for curation; `favorite` remains only as legacy file metadata. |
| Clip | Save | One verb covers pages, media, quotes, and in-app paste/drop without implying that only a fragment is kept. |
| Standalone note | Quote | Source-less text saved to Óia is a quote card, not a separate note kind. |
| Download (user action) | Save | Download describes an implementation detail, not the user's intent. The extension captures live-DOM content and source adapters may retrieve public assets; keep "download" for those technical operations only. |
| Bookmark (user action) | Save | A full browser capture stores cleaned content; a link saved without cleaned content, from the app or toolbar, is explicitly lightweight and can later be upgraded. The bookmark glyph as brand iconography is fine; the verb is not. |
| Plugin | Extension | Browsers and their stores call them extensions. |
| Read-later app | Inspiration library | The product is organized around collecting and revisiting inspiration, not clearing an unread queue. |
| Preferences | Settings | macOS renamed Preferences to Settings; the app's UI says Settings. |
| Web reader | Reader | The macOS reader is native, not WebView-based. |
| Add Link | Paste or drop a link; Save | The app uses the standard paste/drop gestures rather than a bespoke form, and the user-facing action is still Save. |

## Flagged Ambiguities

- **"Save" vs "capture".** The *user saves* a page; the *extension captures*
  it (extraction, cleaning, image bytes) as the technical step inside that
  save.
- **"Inspiration" as identity.** It describes why mixed pages, images, videos,
  and quotes belong together. It does not imply that every card must be visually
  decorative or that articles stop being readable.
- **"Bookmark" is overloaded.** It is the brand glyph, an Apple API term in the
  macOS client (security-scoped bookmarks), and a rejected user-facing verb.
  Only the first two uses are legitimate.
- **"Reading" remains the internal domain entity.** Renaming the storage model is
  a separate format/API migration. User-facing copy says card, saved item, or the
  concrete kind so the product identity stays broader than articles.

## Example Dialogue

> **Dev:** "When the user **saves** a page, does the **extension** write the
> **article file**?"
>
> **Domain expert:** "No — the extension only **captures**: it extracts and
> cleans the live DOM and gathers image bytes. The **native messaging host**
> writes the **reading** into the **library** through the **core**."
>
> **Dev:** "How does someone organize what they saved?"
>
> **Domain expert:** "Everything stays together on the board. Óia adds local subject
> **Tags** to saved items, and the user can add or remove Tags, search for an item later,
> or permanently **delete** it when it no longer belongs."
>
> **Dev:** "So the **index** knows all of this?"
>
> **Domain expert:** "The index only mirrors it. The frontmatter in the article
> file is the source of truth; the index is disposable and rebuilt from files."
