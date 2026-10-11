# Inbox capture format

Inbox is a temporary handoff, not another board scope or a second library format.
The shared Rust importer writes ordinary format-v1 readings; the Shortcut never
writes `article.md` or assigns reading ids.

## Publication

The iOS Shortcut creates one uniquely named `<capture-id>.cuttingscapture.zip`
per shared item and saves it to `<library>/inbox/`. Publish a complete archive;
never append to or overwrite a published capture. One ZIP keeps metadata and
payloads together even when an external sync service delivers files out of order.
No companion manifest or completion marker is needed outside the archive.

The archive contains a root `manifest.json` and any referenced attachment files.
The current Shortcut writes version 4 for ordinary captures:

```json
{
  "version": 4,
  "capture_type": "selection",
  "capture_id": "unique-transport-identifier",
  "captured_at": "2026-09-10T12:00:00Z",
  "origin": {
    "url": "https://example.com/page",
    "title": "Page title",
    "canonical_url": "https://example.com/canonical",
    "site_name": "Example"
  },
  "text": "Selected text"
}
```

`version`, `capture_type`, `capture_id` (nonempty, at most 128 bytes), and RFC3339
`captured_at` are required. The type describes what the sharing app supplied,
not the eventual reading kind or category. Rust owns URL recognition, metadata
classification and selected-text routing, as it does for browser saves.

| `capture_type` | Required payload | Import behavior |
|---|---|---|
| `article` | `origin.url`, or an exact HTTP(S) URL in `text`; optional `article` HTML file and image attachments | Preserves shared source/category resolution, then saves usable supplied prose as a full article. URL-only requests attempt bounded public-page extraction on the Mac and retain a link when extraction is unavailable or unsuitable. |
| `url` | `origin.url`, or an exact HTTP(S) URL in `text`; no attachments or other text | Uses the shared URL-save facade, including recognized public source retrieval and metadata classification. Books, movies, music and products receive supported category metadata, covers and prices. Other URLs remain lightweight links. |
| `selection` | Nonempty `text` (at most 64 KiB) and `origin.url`; optional article HTML and its images in version 4 | Uses the shared selection router: text enclosed in matching quotation marks becomes a quote; other passages become attached highlights. Supplied source prose can upgrade the enclosing article before attaching a highlight. |
| `text` | Nonempty `text`; no source URL or attachments | A standalone quote, unless the entire text is an HTTP(S) URL, in which case it uses the URL-save facade. |
| `media` | One or more `attachments`; no text | Imports the shared files with their available origin metadata. |

Unknown types and incompatible payload/type combinations are rejected and
retained in Inbox. Producers must not guess `book`, `movie`, `tweet` or
`highlight` as a capture type. That decision belongs to the shared Rust engine.
Capture time is normalized to the library's fixed-width UTC date representation.

An article body stays outside the manifest so large content does not exceed its
1 MiB bound. Safari Page Contents and native Article Body are converted to HTML
by Apple's content actions and included in the archive:

```json
{
  "version": 4,
  "capture_type": "article",
  "capture_id": "unique-transport-identifier",
  "captured_at": "2026-10-11T12:00:00Z",
  "origin": {"url": "https://example.com/article", "title": "An article"},
  "article": {
    "path": "article.html",
    "format": "html",
    "sha256": "64 hexadecimal characters"
  }
}
```

The HTML must be UTF-8 and at most 8 MiB. A supplied `byte_count` or `sha256`
must match; the Shortcut always hashes its HTML. Rust converts semantic article
content to Markdown, resolves relative links against the page origin, and
derives classification evidence. It ignores navigation, hidden content and
scripts. Supported embedded image bytes become local assets. External images
without supplied bytes become labelled offline placeholders through the normal
article-save pipeline; they are not silently treated as local captures.

Article image attachments may include `source_url` and `content_type` alongside
their path and optional checksum/size. Article image mappings require an HTTP(S)
source URL and valid image bytes, with at most 64 images and 40 MiB combined
decoded supplied image data. HTML rendering is bounded to 64 nesting levels;
metadata evidence retains the classifier's separate 512 KiB bound.

A media manifest contains its file references and checksums:

```json
{
  "version": 4,
  "capture_type": "media",
  "capture_id": "unique-transport-identifier",
  "captured_at": "2026-09-10T12:00:00Z",
  "attachments": [
    {
      "path": "payload.jpg",
      "byte_count": 12345,
      "sha256": "64 hexadecimal characters"
    }
  ]
}
```

The example checksum is descriptive; producers supply the actual SHA-256. The
shipped Shortcut always hashes media; the importer also accepts ZIP producers
that omit the optional byte count/hash, using the ZIP sizes and CRC validation.

`capture_id` identifies transport only. Normal content-addressed reading ids
still control deduplication. A source must be HTTP(S); local paths never become
an origin. A canonical URL is metadata and does not replace the visited URL.

Version-4 article requests with no supplied body can retrieve public HTML on the
Mac, bounded to 8 MiB, eight seconds and three redirects using the existing
public-source transport. Retrieval failure or insufficient article evidence
keeps the lightweight link. Earlier versions and ordinary app URL saves retain
their existing behavior. Available title/site/canonical data is retained.
The Shortcut does not duplicate Rust's category logic. The browser can
additionally report `<q>`/`<blockquote>` enclosure; Shortcuts supplies no such
selection DOM evidence.

## Typed ordinary captures (version 3)

Version 3 remains supported with `url`, `selection`, `text` and `media` types.
It has no article HTML field or generic full-page retrieval. Selections have no
attachments, and ordinary URLs retain the shared URL-save facade's lightweight
fallback.

## Legacy ordinary captures (version 1)

Version 1 remains supported with the same metadata, text and attachment fields,
without `capture_type`. Rust infers media from attachments, a selection from text
with a page origin, standalone text from text alone, and a URL save from an
origin alone. An exact HTTP(S) URL in text still uses the URL-save facade when
no different page origin is given. Legacy captures therefore use the updated
shared save behavior after the Mac app is updated.

Earlier readers reject unsupported capture versions and retain the archive.
Update the Mac app before installing the new Shortcut; there is no fallback that could save a
selection as the wrong reading kind.

## Instagram requests (version 2)

An explicit request contains only `manifest.json`:

```json
{
  "version": 2,
  "capture_id": "unique-transport-identifier",
  "captured_at": "2026-09-22T12:00:00Z",
  "instagram_url": "https://www.instagram.com/p/DdlVpikk5Gj/?img_index=2"
}
```

Version 2 is reserved for selected Instagram media requests. It does not accept
attachments or text, and never falls back to a link. Version-1-only readers reject
and retain it. Rust validates the HTTPS Instagram post/reel URL, interprets the
one-based `img_index` (missing means 1), rejects ambiguous/invalid indices, and
removes tracking parameters from the saved origin. The native app opts into the
external downloader. URL captures also use the shared source/category retrieval
path; supplied media and non-URL text import without network access.

Instaloader runs in a per-device Python environment, outside the library. It
fetches exactly the selected node, uses video bytes rather than a poster for a
video node, and streams at most 40 MiB per image or 1 GiB per movie to private
temporary storage. Each helper process has a 120-second deadline. Rust validates
the resulting media through the normal importer and verifies the saved asset
before consuming the unchanged request. The original share time is preserved.
No account sessions, browser cookies, CDN URLs, or credentials are stored in the
request or saved card. Network/authentication failures and missing slide indices
stay in Inbox as issues; **Check Inbox** retries them. See [setup](ios-shortcut.md#instagram).

## Validation and consumption

- Only direct regular files are considered. Hidden files, symlinks, and folders
  are not traversed. The Inbox itself must be a real directory.
- The Mac requests unavailable iCloud items and defers files whose local copy
  is not current. Rust also skips dataless files and files modified within the
  previous two seconds, then takes a bounded private snapshot.
- ZIP entry names must be safe relative paths; duplicates, special files,
  missing attachments, unknown versions, and unreferenced data are rejected.
  Directory entries must have no payload bytes and their file types must match
  their trailing-slash names.
  Entries are copied to generated temporary filenames, never extracted using
  archive paths. ZIP integrity and declared hashes/sizes are checked before
  importing any attachments.
- Limits: 128 archive entries, 64 attachments, 1 MiB manifest, 8 MiB article HTML, 40 MiB per image
  or text file, 1 GiB per video, and 1 GiB + 2 MiB for the archive/expanded total.
  The ZIP manifest limit also bounds text embedded in the manifest.
- Saved Markdown and required asset bytes are verified and flushed before
  removal. A duplicate id alone is not sufficient evidence of a complete save.
  Quote text and source identities are checked too, so valid frontmatter alone
  cannot make a truncated or edited quote safe to discard.
  The original source is atomically renamed, without overwriting, to a unique
  visible `recovered-<id>-<original-name>` inside the pinned Inbox directory.
  Its file identity, size, timestamps, and hash are checked again before removal.
  A replacement or interrupted cleanup leaves that recovery file available for
  the next normal import; a new file at the original name is never removed by
  the old capture's cleanup. Recovery names preserve the supported extension
  (including `.cuttingscapture.zip`) and may shorten an unusually long stem.
- Unsupported, invalid, or failed inputs stay intact in Inbox. The app presents
  issues in Settings and can check again. Pending files retry with bounded
  backoff while the Mac app is running; no daemon runs when the app is closed.

This is at-least-once ingestion: a crash after saving but before removal retries
through normal deduplication. A multi-attachment archive can have some readings
saved before a later write fails; the whole archive remains for retry. The
index is reconciled once after the batch. Local locks do not coordinate Macs;
external sync is still responsible for delivery and conflict handling.

Ordinary JPEG, PNG, GIF, WebP, HEIC/HEIF, MP4/MOV/M4V, UTF-8 `.txt`/`.text`/`.md`,
`.url`, and `.webloc` files may also be dropped directly into Inbox. Use unique
filenames and finish copying before leaving files there; do not use Inbox as a
working folder or edit queued files. Raw text has no sealed completion marker,
so this compatibility path uses local file stability rather than the stronger
archive contract. Raw files have no extra source metadata or captured timestamp.

See [iPhone setup](ios-shortcut.md) for the installable Shortcut and device checks.
