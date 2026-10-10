# Importing a mymind export

This migration adapter reads the folder produced by mymind's **Export my mind** action: a
`cards.csv` file plus the media files beside it. It accepts either that folder directly or an
outer folder containing exactly one immediate child with `cards.csv`. Import planning is offline,
and the importer never writes a SQLite index. Product migration previews fetch live metadata.

From the repository root, preview the complete plan first:

```bash
./scripts/import-mymind.sh \
  --export /path/to/mymind \
  --library /path/to/Óia
```

The preview reads and hashes the exported media but does not fetch any links or change the Óia
library. It reports how many link metadata fetches the subsequent write would attempt. Its default
output contains aggregate counts only. Add `--verbose` to list opaque mymind card IDs and Óia
reading IDs without printing private URLs, tags, notes, content, or source paths.

Review the totals and warnings, then run the same command with `--write`:

```bash
./scripts/import-mymind.sh \
  --export /path/to/mymind \
  --library /path/to/Óia \
  --write
```

Writes enrich HTTP(S) link rows by default. The importer fetches live page metadata plus a bounded
social preview and favicon, then gives the captured metadata and bytes to `oia-core`. This is
the network adapter at the import boundary; ordinary library browsing remains offline.

To prohibit network access and write URL-only lightweight links from the export data, add
`--offline`:

```bash
./scripts/import-mymind.sh \
  --export /path/to/mymind \
  --library /path/to/Óia \
  --write \
  --offline
```

The command is safe to re-run. Core content addressing reports readings already in the destination
as present and leaves their current Óia tags and legacy note sidecars untouched. Planning errors,
including conflicting non-empty notes on duplicate source rows, block all writes.

## Link enrichment

For each web link, the default write path may capture the canonical URL, title, site, author,
language, excerpt, and the website's declared theme colour. The core validates the theme colour and
stores supported values as optional lowercase `#rrggbb` `theme_color` metadata for card
presentation. Unsupported or absent colours are simply omitted.

Fetched social-preview and favicon bytes are stored inside the reading's own `assets/` directory.
Frontmatter refers to them through the local relative `preview_asset: assets/<file>` and
`favicon_asset: assets/<file>` fields, so the resulting card remains usable offline. A page metadata
fetch failure is reported as a warning. Reachable pages that reject inspection fall back to the
export's URL and title; a confirmed 404/410 or an origin that remains unreachable after a retry is
not imported. An unavailable preview or favicon is simply omitted.

### Enriching links already in a library

The separate migration script snapshots lightweight links that have neither a preview nor favicon.
It previews by default and reports an aggregate count plus a digest of sorted opaque reading IDs:

```bash
./scripts/enrich-link-metadata.sh --library /path/to/Óia
```

Apply the same snapshot with `--write`. For a one-off audited migration, `--expect-count` and
`--expect-digest` make the command abort before network access or writes if the target changed:

```bash
./scripts/enrich-link-metadata.sh \
  --library /path/to/Óia \
  --write \
  --expect-count 123 \
  --expect-digest <sha256>
```

Successful captures enrich the existing files while preserving their body, save date, user state,
tags, and note sidecar. During this explicit existing-library migration, links returning HTTP 404 or
410 are permanently deleted. Origins still unreachable after two attempts are also deleted unless
the batch resembles a local or network-wide outage (no server was reached, or more than a quarter
of a batch of at least 20 links was unreachable). Rejected, rate-limited, oversized, and non-HTML
responses are reachable and remain in the library.

### Identifying existing product links

Product metadata is separate from the Reading kind: an imported product retains its lightweight
article, original URL-derived ID, local preview assets, and body. Link imports capture the same
structured product evidence during ordinary enrichment.

To inspect existing lightweight links for product metadata, run:

```bash
./scripts/import-mymind.sh \
  --library /path/to/Óia \
  --enrich-existing-products
```

This preview fetches bounded public HTML and reports the eligible count, target digest, detected
products, and prices. It includes links that already have previews; existing product metadata,
social source profiles, media, quotes, and local identities are excluded. Detection uses the shared
core parser for schema.org Product/ProductGroup JSON-LD, scoped microdata/RDFa, or explicit product
Open Graph metadata. A price or product-looking URL alone is insufficient. A product without a
valid price remains a product with no price badge.

Apply with an empty backup folder outside the library:

```bash
./scripts/import-mymind.sh \
  --library /path/to/Óia \
  --enrich-existing-products \
  --write \
  --backup-dir /path/to/product-metadata-backup \
  --expect-count 123 \
  --expect-digest <sha256>
```

Only product frontmatter is updated. Prices are snapshots in their declared currency, with a UTC
capture date; later browsing requires no network. Product fetches validate public DNS, use bounded
redirects and response sizes, and cap each origin at two concurrent fetches. Fetch failures leave
readings untouched. Files edited while a fetch runs are skipped after a locked fingerprint check.

The backup contains each changed original as `<reading-id>.md` plus `report.txt`. To undo a change,
restore that file to `articles/<first-two-id-characters>/<reading-id>/article.md` and let the app
reconcile its disposable index. Save a current copy first if the reading has been edited since the
migration. Product migration never deletes unavailable readings or rewrites their assets or notes.

### Categorizing existing books, music, and movies

Preview cultural categories for every eligible lightweight article, including products:

```bash
./scripts/import-mymind.sh \
  --library /path/to/Óia \
  --enrich-existing-categories \
  --workers 24
```

The core checks the page's main structured Book, MusicAlbum, MusicRecording, MusicPlaylist, or
Movie entity, exact Open Graph types, and validated ISBN evidence. Narrow provider routes can
establish a category even when their HTML is unavailable. Reviews, collections, recommendation
lists, and generic price or title text do not establish a cultural category. Existing categories,
social source profiles, full articles, media, quotes, and local identities are skipped.

Apply the preview's count and digest with a new empty backup directory outside the library:

```bash
./scripts/import-mymind.sh \
  --library /path/to/Óia \
  --enrich-existing-categories \
  --write \
  --backup-dir /path/to/category-metadata-backup \
  --expect-count 123 \
  --expect-digest <sha256>
```

The migration adds `link_category` while preserving product prices, every other frontmatter
field, raw Markdown, IDs, tags, notes, and user state. A full-file snapshot is checked under the
reading's lock and again before committing. Concurrent changes are skipped, and unavailable
pages are retained. A repeated migration skips previously categorized cards.

To apply only cards recognized in a reviewed preview, add `--only-ids-file /path/to/ids.txt`.
The file must contain one lowercase 64-character reading ID per line. Empty selections, malformed
or duplicate IDs, and IDs that are now missing or ineligible abort before any fetch or backup.
Count and digest guards apply to the selected subset, with IDs sorted and separated by newlines
including a final newline. The default still inspects all eligible links.

With `--verbose`, unavailable readings include a short root error reason, such as a connection
timeout or an HTTP status. Requested URLs and the surrounding request context are omitted.

Artwork is downloaded only during a write, when the reading has no usable regular local preview.
Bounded public requests reject private addresses, unsafe redirects, unsupported images, and
oversized responses. The artwork gets a new immutable `assets/category-artwork-<sha256>.<format>`
file; existing assets are never replaced. Missing artwork does not prevent categorization.

Original files are saved as `<reading-id>.md`. An atomically updated `report.txt` records each
new asset's relative path and hash. If a concurrent writer changes the article after artwork is
installed, the new asset is retained and reported, since that writer might already reference it.
To undo a category, restore its backed-up article after preserving any newer user edits. Remove
new artwork only after checking that no current article refers to it. The app rebuilds its index
from the restored files.

## Mapping

The observed mymind CSV columns are
`id,type,title,url,content,note,tags,created`. The importer maps them as follows:

| mymind export | Óia |
| --- | --- |
| Web card with an HTTP(S) URL | Lightweight article, enriched on write unless `--offline`, and ready for a later full browser capture |
| `Content` snippet with text | Quote, retaining its HTTP(S) origin when present |
| `Note` content | Source-less quote |
| Image/video file whose stem exactly matches the row ID | Local media card; HTTP(S) origin retained when present |
| `note` attached to a row | Legacy `note.md` sidecar, preserved in the files but not shown by the current macOS app |
| Comma-separated `tags` | Lowercase, hyphenated Óia tags |
| `created` | Original UTC `saved_at` |

Media is identified by its bytes, not its opaque export filename or a repeated URL. Rows that map
to the same Óia identity are coalesced before writing: the earliest save date is retained, the
latest non-empty title is used, and valid tags are combined. This prevents duplicate handling from
silently dropping source metadata.

Tags over Óia' 20-character limit are counted and omitted rather than truncated. Óia has
no PDF card kind, so exported PDF bytes are counted as skipped; a document's valid source URL is
still preserved as a lightweight article. Source-less screenshots and empty quotation rows with no
surviving source URL are also skipped because this export does not contain their missing payloads.
Symbolic links, unknown file types, and source paths outside the export folder are never imported.

The source export layout is documented by
[mymind's export guide](https://mymind.helpscoutdocs.com/article/18-can-i-export).
