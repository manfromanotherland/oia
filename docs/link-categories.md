# Link categories

Book, Music, and Movie are presentation categories of article readings, alongside Product. They
retain the URL-derived reading identity and appear in Links. Search accepts `book`, `music`,
`movie`, and `product` as item-type terms, composing with remaining free text, Tags, and board
scope. Subject Tags stay independent: categorization does not add or remove a Tag.

The optional `link_category` frontmatter string holds `book`, `music`, or `movie`. A Book sold in a
shop uses Book presentation and search while retaining any Product price snapshot. SQLite and
native rows are disposable projections of that file metadata; unsupported category strings survive
file round trips and fall back to ordinary presentation.

## Recognition

Rust selects the page's main entity from Schema.org JSON-LD graphs, scoped microdata/RDFa, or
explicit Open Graph types. The recognized types include Book, MusicAlbum, MusicRecording,
MusicPlaylist, and Movie; Open Graph offers `book`, `music.album`, `music.song`, `music.playlist`,
and `video.movie`. A main Product with a valid ISBN can identify a Book. Graph references and
canonical identity associate evidence with the page; incidental recommendations, reviews, and
collections must not change its category.

Precise provider entity routes provide conservative fallback evidence for book, music, and film
detail pages. A generic shop route, words in a title, or a price alone cannot identify the category.
IMDb title routes require typed evidence because the same pattern also identifies TV shows and
episodes. When an exact title page has no usable entity metadata, optional retrieval can consult
IMDb's public suggestion catalog. Only one result matching the requested title ID with `qid: movie`
supplies Movie evidence and a poster. Explicit article, review, collection, or television metadata
and excessive page metadata prevent this fallback. Artist pages, podcasts, generic YouTube videos,
and book collections are not automatically Music or Movie items.

The extension sends bounded raw live metadata through the existing `product_html` protocol field.
The pure `inspect_page_metadata` request lets Rust choose an exact artwork URL before the
extension fetches its bytes. Final save requests revalidate category evidence in Rust. An older or
unavailable host falls back to ordinary preview capture. URL-only saves use bounded optional
retrieval through the shared Rust facade, and remain useful offline.

## Artwork and existing links

Selected entity artwork is preferred to a generic page banner and saved into the reading's local
`assets/` folder. Books use a shaded spine, Music a square sleeve with a record behind it, and
Movies a perforated poster. Missing artwork leaves a readable category/title card. Native browsing
does not fetch remote artwork.

The explicit `--enrich-existing-categories` migration scans existing lightweight links, including
Products. Preview does not write library files. Applying it requires a separate backup folder,
preserves raw original files, and revalidates each reading fingerprint before an atomic metadata
update. Existing usable previews are retained; missing covers may be captured locally. Uncertain
or inaccessible pages remain unchanged unless a precise entity route supplies sufficient evidence.
The report records categories, changed files, artwork, and skipped or unavailable pages. Nothing
is deleted.

## Sources

[Schema.org Book](https://schema.org/Book), [MusicAlbum](https://schema.org/MusicAlbum),
[MusicRecording](https://schema.org/MusicRecording), and [Movie](https://schema.org/Movie) define
the relevant entities. The [Open Graph specification](https://ogp.me/) defines typed pages and their
artwork. The [IMDb public suggestion response](https://v3.sg.media-imdb.com/suggestion/t/tt0098554.json)
provides an exact movie type, title, and poster. This endpoint was observed on 10 October 2026;
it is not a documented stable API, and failed or unknown responses leave the link unclassified.
These are our detection choices; mymind does not publish its recognition algorithm.
