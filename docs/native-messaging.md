# Native Messaging Protocol

**Version:** `4`

This document defines the JSON message contract between the **browser extension**
(`extension`) and the **native messaging host** (`core/native-host`).

The host is registered under the name `is.edmundo.cuttings.host`. Communication uses the standard
[Chrome native messaging protocol][chrome-nm]: each message is prefixed with a 4-byte
little-endian unsigned integer specifying the message length in bytes.

- **Browser → host:** up to 64 MiB in Chromium browsers; Firefox permits up to 4 GB.
- **Host → browser:** up to 1 MB per message (browser-enforced hard limit).

[chrome-nm]: https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging

---

## Save request (extension → host)

Sent when the user saves a full article, screenshot, right-clicked image, or selected-text quote.
Every right-clicked video uses the streaming import below. The request
always describes the **origin page** in `metadata.url`/`canonical_url`; a media identity is
additional metadata and never replaces that origin.

```json
{
  "protocol_version": 4,
  "action": "save",
  "metadata": {
    "url": "https://blog.example.com/posts/local-first?utm_source=hn",
    "canonical_url": "https://blog.example.com/posts/local-first",
    "title": "Local-First Software",
    "kind": "article",
    "author": "Martin Kleppmann",
    "site": "blog.example.com",
    "theme_color": "#123456",
    "lang": "en",
    "excerpt": "An argument for software that works offline.",
    "word_count": 3812,
    "saved_at": "2026-06-13T15:00:00Z"
  },
  "markdown": "## Local-First Software\n\nAn argument…\n\n![Diagram](https://blog.example.com/img/diagram.jpg)\n",
  "images": [
    {
      "url": "https://blog.example.com/img/diagram.jpg",
      "content_type": "image/jpeg",
      "data_base64": "/9j/4AAQSkZJRg…"
    },
    {
      "url": "https://blog.example.com/social-card.jpg",
      "content_type": "image/jpeg",
      "data_base64": "/9j/4AAQSkZJRg…"
    },
    {
      "url": "https://blog.example.com/favicon.ico",
      "content_type": "image/x-icon",
      "data_base64": "AAABAAEA…"
    }
  ],
  "preview_url": "https://blog.example.com/social-card.jpg",
  "favicon_url": "https://blog.example.com/favicon.ico"
}
```

### Fields

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `protocol_version` | integer | ✅ | Must be `4` |
| `action` | string | ✅ | `"save"` for this request; `"check"` is also supported (see below) |
| `metadata.url` | string | ✅ | Original URL as visited |
| `metadata.canonical_url` | string | ✅ | Origin page's canonical URL when known; otherwise the visited page URL |
| `metadata.title` | string | ✅ | Origin page/article title |
| `metadata.kind` | string | ✅ | `article`, `image`, `video`, or `quote` |
| `metadata.media_url` | string | — | Media identity in an ordinary image or legacy video save. Current browser videos use the streaming import below and omit this field at begin |
| `metadata.author` | string | — | Extracted byline; omit if unknown |
| `metadata.site` | string | — | Source-site label, usually the page's Open Graph site name or hostname; omit if unknown |
| `metadata.theme_color` | string | — | First non-empty page `theme-color`/`theme_color` meta value; core validates and normalizes supported CSS colours before storing it |
| `metadata.product_html` | string | — | Bounded raw live-DOM product evidence (JSON-LD, scoped microdata/RDFa, product meta tags, canonical identity). Rust validates/classifies article and link saves; standalone media and quotes omit it. Extension cap: 512 KiB UTF-8. Additive to protocol v4. |
| `metadata.lang` | string | — | BCP-47 language tag |
| `metadata.excerpt` | string | — | One-sentence summary |
| `metadata.word_count` | integer | — | Word count of the cleaned body |
| `metadata.saved_at` | string | ✅ | ISO-8601 UTC timestamp from the extension |
| `markdown` | string | ✅ | Cleaned article body as Markdown; images still have remote URLs |
| `images` | object[] | ✅ | Article images or a clicked image captured by the extension; may be empty |
| `images[].url` | string | ✅ | Source lookup key. Body-image URLs also appear in `markdown`; role-only URLs match `preview_url` or `favicon_url` |
| `images[].content_type` | string | — | The response `Content-Type` the browser saw; used to pick the asset extension |
| `images[].data_base64` | string | ✅ | Standard base64 of the raw image bytes |
| `preview_url` | string | — | Captured social/meta image URL to promote to the local `preview_asset` without inserting it into Markdown |
| `favicon_url` | string | — | Captured page favicon URL to retain as the local `favicon_asset` |

The extension fetches each article image or clicked image itself — from the browser's
cache where possible — so the ordinary capture writer never re-fetches it. An image the extension
couldn't capture is simply omitted from `images`; its URL stays in Markdown and the reader shows a
labelled placeholder. A strictly recognized article URL is first offered to the separate Rust source
adapter. If that adapter cannot retrieve or validate a complete source capture, a full `save` request
falls back to the Markdown and image bytes the extension already captured; storage failures still
fail the request. Ordinary save messages never contain video bytes; every current browser video uses
the streaming import below and is successful only after its local movie asset is committed.

For articles, the extension also inspects Open Graph/Twitter metadata and the browser-selected or
declared favicon, plus an optional website theme colour. Successfully captured role assets are
written locally and referenced from frontmatter; they are never injected into the cleaned article
body. The core normalizes a supported theme colour into the library's canonical form. For a quote,
`markdown` contains the full selected text as block quotes and `excerpt` contains the bounded card
preview. For image/video/quote requests, the host derives a kind-specific identity so several cards
can coexist from one origin page.

---

## Save link request (extension → host)

`save_link` first passes the URL through the shared Rust URL-save facade. A recognized public source
is resolved into a complete local article; an unrecognized URL stores a lightweight link: page
metadata and local preview/favicon assets are retained, but no cleaned article body is claimed. The
core constructs that link body and marks the reading `lightweight: true`, so a later full article
capture upgrades the same URL-derived reading. Unlike a full `save`, `save_link` has no cleaned body
to fall back to: failure to retrieve or validate a recognized source returns `source_unavailable`
and does not write a misleading lightweight card.

For ordinary product pages the extension includes `metadata.product_html`, using metadata already
present in the live page. The host parses it with the shared Rust product parser and writes the
optional versioned `product` frontmatter without changing the URL-derived reading ID. No extra
host page request is necessary. Product recognition and pricing never depend on JavaScript
duplicating the Rust rules; an absent or invalid product payload leaves an ordinary link.

```json
{
  "protocol_version": 4,
  "action": "save_link",
  "metadata": {
    "kind": "article",
    "url": "https://example.com/post?utm_source=feed",
    "canonical_url": "https://example.com/post",
    "title": "Page title",
    "site": "Example",
    "theme_color": "#123456",
    "excerpt": "Open Graph description",
    "saved_at": "2026-08-25T09:00:00Z"
  },
  "images": [
    {
      "url": "https://example.com/social-card.jpg",
      "content_type": "image/jpeg",
      "data_base64": "/9j/4AAQSkZJRg…"
    },
    {
      "url": "https://example.com/favicon.ico",
      "content_type": "image/x-icon",
      "data_base64": "AAABAAEA…"
    }
  ],
  "preview_url": "https://example.com/social-card.jpg",
  "favicon_url": "https://example.com/favicon.ico"
}
```

The `metadata`, `images`, `preview_url`, and `favicon_url` fields have the same meanings as in a
normal save request. `metadata.kind` must be `article`. If preview metadata cannot be read, the
extension may still send tab title/URL metadata with an empty image list.

---

## Browser video import (extension ↔ host)

Every browser video save uses this path. The extension streams readable HTTP(S), `data:`, or
document-scoped `blob:` bytes from the live page. When a source cannot be fetched, it records one
loop of the exact rendered element using an explicitly supported H.264 MP4 recorder. It streams
either result through one persistent native-messaging connection. The background worker only
relays messages; the native host performs no network request. A poster-only response is an error,
not a successful video save.

The connection accepts one active upload. Every begin and chunk is acknowledged before the
extension sends the next message, so neither side accumulates the complete video in memory. Each
decoded chunk is non-empty and at most 256 KiB; the complete decoded video is at most 1 GiB.

### Begin

```json
{
  "protocol_version": 4,
  "action": "video_import_begin",
  "upload_id": "0f6774ea-d99d-49d7-a1ad-7fd295fe6aa4",
  "metadata": {
    "kind": "video",
    "url": "https://www.cosmos.so/e/2035271300",
    "canonical_url": "https://www.cosmos.so/e/2035271300",
    "title": "Saved video",
    "site": "Cosmos",
    "theme_color": "#123456",
    "saved_at": "2026-08-25T12:00:00Z"
  },
  "content_type": "video/mp4",
  "expected_bytes": 7340032
}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `protocol_version` | integer | ✅ | Must be `4` |
| `action` | string | ✅ | `video_import_begin` |
| `upload_id` | string | ✅ | Non-empty identifier used by every message in this connection-scoped upload |
| `metadata` | object | ✅ | Ordinary origin metadata with `kind: "video"`; `media_url` is omitted because the core derives the local asset identity after hashing |
| `content_type` | string | ✅ | Supported video MIME type used for the final safe extension |
| `expected_bytes` | integer | — | Exact total decoded byte count when known; must be between 1 byte and 1 GiB |

The host creates a unique file in `.cuttings-imports/`, initializes the incremental SHA-256, and
responds with a small acknowledgement:

```json
{
  "protocol_version": 4,
  "ok": true
}
```

### Chunk

```json
{
  "protocol_version": 4,
  "action": "video_import_chunk",
  "upload_id": "0f6774ea-d99d-49d7-a1ad-7fd295fe6aa4",
  "sequence": 0,
  "data_base64": "AAAAHGZ0eXBpc29tAAACAGlzb20…"
}
```

`sequence` starts at `0` and increases by exactly one. `data_base64` is standard base64 whose
decoded value contains 1–262,144 bytes. The host writes and hashes the decoded chunk, then returns
the acknowledgement above. The extension waits for it before sending the next sequence.

### Finish

```json
{
  "protocol_version": 4,
  "action": "video_import_finish",
  "upload_id": "0f6774ea-d99d-49d7-a1ad-7fd295fe6aa4"
}
```

Finish rejects an empty stream or a total that differs from `expected_bytes`. On success, the core
uses the raw-byte SHA-256 to commit `assets/<sha256>.<ext>`, writes a content-derived
`cuttings-asset:` media identity, and returns the normal save-success response with `id` and
`path`. Re-importing identical bytes from the same normalized origin returns the normal
`duplicate` error, allowing the extension to present it as “Already in Óia.” Identical bytes
from another origin produce a different reading id.

### Abort and cleanup

```json
{
  "protocol_version": 4,
  "action": "video_import_abort",
  "upload_id": "0f6774ea-d99d-49d7-a1ad-7fd295fe6aa4"
}
```

An accepted abort returns the acknowledgement shape. Abort, port disconnect/EOF, malformed base64,
an empty or oversized chunk, a wrong upload id or sequence, a second begin, an unrelated action,
size mismatch, and any storage error all terminate the active upload and remove its incomplete
staging file. A later upload on a new or recovered connection starts again at sequence `0`.

---

## Save response (host → extension)

### Success

```json
{
  "protocol_version": 4,
  "ok": true,
  "id": "1146c9a93631d1991af3252dbc49ecd8043ab354a4386e397d555d1ca21a7199",
  "path": "articles/11/1146c9a93631d1991af3252dbc49ecd8043ab354a4386e397d555d1ca21a7199/article.md"
}
```

### Failure

```json
{
  "protocol_version": 4,
  "ok": false,
  "error": "library_not_configured",
  "message": "No library folder has been set. Open the Óia app to configure one."
}
```

### Response fields

| Field | Type | Notes |
|-------|------|-------|
| `protocol_version` | integer | Always `4` |
| `ok` | boolean | `true` on success |
| `id` | string | The content-addressed id assigned to the reading (success only) |
| `path` | string | Relative path from library root (success only) |
| `error` | string | Machine-readable error code (failure only) |
| `message` | string | Human-readable description (failure only) |
| `saved` | boolean | Whether the URL is already saved (`check` responses only) |

### Error codes

| Code | Meaning |
|------|---------|
| `library_not_configured` | No library folder set; user must open the app |
| `duplicate` | A save, including a completed streamed-video import, found a card with the same deterministic identity |
| `source_unavailable` | A recognized URL-only source could not be retrieved or validated; no lightweight fallback was written |
| `io_error` | Failed to write to the library folder |
| `invalid_request` | Malformed or missing required fields |

---

## Check request (extension → host)

Asks whether a URL is already in the library — used to reflect saved state in the toolbar.

```json
{
  "protocol_version": 4,
  "action": "check",
  "url": "https://blog.example.com/posts/local-first"
}
```

The host looks up the article identity for that normalized page URL. For a recognized source it also
matches provider aliases by the stable `source_profile` provider/source id. A full article or
lightweight link counts as saved; saving only an image, video, screenshot, or quote from the page
does not make the article icon appear saved.

```json
{
  "protocol_version": 4,
  "ok": true,
  "saved": true,
  "id": "1146c9a93631d1991af3252dbc49ecd8043ab354a4386e397d555d1ca21a7199"
}
```

`saved` is `true` when a reading with that URL exists (`id` is then its content-addressed id), and `false`
otherwise — including when no library is configured (a `check` never errors on that).

---

## Protocol versioning

- Version `1` supported full-page article saves. Version `2` added first-class card kind and media
  metadata for articles, images, videos, and quotes. Version `3` added lightweight browser-link
  saves plus explicit local social-preview and favicon asset roles. Version `4` adds acknowledged,
  bounded streaming imports for browser videos. Optional `metadata.theme_color` is an additive
  version-4 field; senders and receivers may omit it. Provider-aware URL resolution is a host-side
  save behavior and does not change the wire shape.
- The extension and host must both support the same version; a mismatch should surface an error.
- Version bumps follow the same lockstep rule as the library format: update both components in one
  monorepo commit.
