# DESIGN.md — Óia

> UI/UX design for the whole **Óia ecosystem** — the macOS app and browser extension. The
> shared identity (product name and logo) applies to every surface. The browser extension uses the
> brand palette below; the Mac client uses Apple semantic colors and native controls so it follows
> the current macOS appearance and accessibility settings. Architecture and data principles live
> in [AGENTS.md](./AGENTS.md).

## Source

- [`assets/icon.png`](./assets/icon.png) — the app logo / icon.

## Product name

The product name is **Óia**. It is displayed to end users as "Óia."

## Logo / app icon

<img src="./assets/icon.png" alt="Óia app icon" width="96" />

The app icon is a rounded-square ("squircle") in the same near-black charcoal as the UI, with a
centered cream/off-white **bookmark** glyph (notched bottom). It ties directly into the design
language — dark, minimal, content-first — and reuses the **bookmark motif** that also appears in
the app's empty state ("Your reading list is empty").

- **Source:** [`assets/icon.png`](./assets/icon.png).
- **Usage:** the macOS app icon, and the basis for the browser-extension toolbar icon and any
  favicon — ideally a monochrome glyph (cream-on-dark / dark-on-light) at small sizes.
- **To finalize:** export the full macOS icon set (`.icns` / `AppIcon.appiconset`, 16–1024 px)
  and the extension icon sizes (16 / 32 / 48 / 128 px); confirm the glyph stays legible at 16 px.

## Brand color palette

The browser extension, its install/options pages, and the in-page save toast use the **paper**
(light) and **charcoal** (dark) palette below. The app icon uses the same brand neutrals.

The macOS app does **not** reproduce these values as a custom UI skin. It uses dynamic AppKit and
SwiftUI semantic colors such as window, control, text, and separator colors. This lets System,
Light, and Dark appearance, increased contrast, and future macOS changes work without a parallel
theme implementation.

The identity is **paper + ink** — warm off-white and near-black. **The primary action is dark ink,
never blue.** The only non-gray brand accent is marker **yellow**; status colors (green / amber)
are functional signals that sit *outside* the brand neutrals.

| Token | Role | Paper (light) | Charcoal (dark) |
|-------|------|---------------|-----------------|
| `bg` | base background | `#fdfcfb` | `#0d0d0f` |
| `surface` | raised surface / muted bg | `#f6f5f4` | `#161618` |
| `surface-2` | hover fill / inset | `#eeedec` | `#1c1c1f` |
| `fg` | primary text | `#17181a` | `#f1f0ee` |
| `fg-muted` | secondary text | `#55565a` | `#9a9a9e` |
| `fg-subtle` | tertiary text, eyebrows | `#85868b` | `#6c6c70` |
| `line` | borders / dividers | `rgb(23 24 26 / 0.12)` | `rgb(255 255 255 / 0.1)` |
| `line-strong` | stronger borders | `rgb(23 24 26 / 0.18)` | `rgb(255 255 255 / 0.15)` |
| `accent` | primary action ("ink pill") | bg `#17181a` / text `#f7f6f4` | bg `#e8e9eb` / text `#17181a` |
| `highlight` | marker yellow (brand accent) | `#ffe066` | `#ffe066` |

- **The accent inverts between themes** — a dark pill on paper, a light pill on charcoal — and its
  text color inverts with it. It is never a colored (blue) accent.
- **`highlight` is fixed across both themes:** the marker is yellow on paper and charcoal.
- The extension keeps a functional **green** (connected) and **amber** (warning) for status dots
  and log lines. These are signals, not brand colors, and are deliberately the only hues outside
  the neutrals + the highlight accent.

## Design language

- **Theme:** **System** by default, with explicit **Light / Dark / System** options.
- **Tone:** minimal, calm, content-first. Generous negative space; the reading list and reader
  are the focus and the chrome stays quiet.
- **Shape & depth:** use standard macOS window, toolbar, menu, popover, sheet, and control
  treatments. Cards may be rounded because they represent content, but app chrome does not invent
  its own pills, rails, shadows, or selection styles.
- **Typography:** San Francisco through semantic system text styles for app chrome. The reader's
  font family, size, width, and line height remain user-adjustable (see Settings).
- **Native feel:** follow the macOS Human Interface Guidelines and prefer SwiftUI/AppKit controls
  over custom-drawn replacements.

### macOS semantic styling

- Use semantic colors (`windowBackgroundColor`, `controlBackgroundColor`, `textBackgroundColor`,
  `labelColor`, `secondaryLabelColor`, `separatorColor`) instead of fixed light/dark RGB values.
- Use native `Label`, `List`, `Menu`, `Picker`, `Button`, `searchable`, and split-view spacing and
  focus behavior. Do not override their metrics solely to mimic a web design.
- Reserve fixed black/white treatments for content that requires guaranteed contrast, such as a
  video poster scrim and its play symbol.

## App layout — visual card board

The fork replaces the old three-column sidebar/list shell with a search-first visual library. The
user-supplied mymind screenshots are a reference for **how mixed image, quote, video, and article
cards organize into masonry columns**. The surrounding mymind branding and chrome are not copied.

```
┌─────────────────────────────────────────────────────────────────┐
│ Óia      [Labeled board filters] [− +]   [ Search ]      │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────┐ ┌───────┐ ┌─────────┐ ┌──────────┐                │
│  │ quote   │ │ image │ │ video   │ │ article  │                │
│  │         │ └───────┘ │ poster  │ │ preview  │                │
│  └─────────┘ ┌───────┐ └─────────┘ └──────────┘                │
└─────────────────────────────────────────────────────────────────┘
```

### Navigation and search

- Use one full-width board with no sidebar or navigation rail. Keep the first masonry row inset
  from the toolbar by the same 30 pt used at the board's horizontal edges.
- Put the native search field in the unified window toolbar using `.searchable`, with the prompt
  *"Search Óia"*. Native token suggestions can narrow the board to exact tags or an item type;
  completed terms remain value-only pills in that one field. Do not create a
  bespoke `NSSearchField`, duplicate search control, or oversized page header.
- The empty suggestion menu offers Image, Video, Article, Link, Quote, Book, Music, Movie, and
  Product. Selecting a completion
  creates a type token and replaces any earlier type token. Typing a type word or `#RRGGBB` also
  filters by that type or palette color; pressing Return turns complete type and hex terms into
  tokens, while remaining words stay broad free text. Only the last type term applies. Never infer
  an exact tag merely because a tag has the same spelling. Article excludes lightweight links,
  captured social posts, and categorized links; Link selects all three. Book, Music, Movie, and
  Product select their corresponding link categories without creating subject Tags.
- Use one native labeled segmented picker for the board scope, in this order: **All, Images,
  Videos, Articles, Links, Quotes**. Images and Videos each show only their matching card kind;
  Articles includes longform X Articles but excludes lightweight link placeholders and captured
  social posts and categorized links. Links includes lightweight URL saves, captured social posts,
  and categorized links. Exact tag filtering
  comes from native suggestions in the toolbar search field.
- Within **Images**, a secondary native segmented button group selects **All images**,
  **Hide photography**, or **Photography**. Photography depicts real-world scenes and objects;
  Hide photography excludes cards identified as photography while retaining graphics,
  screenshots, illustrations, and images without a confident assessment. The choice composes
  with search and subject Tags such as interiors or gardens, is remembered when switching
  board scopes, and only applies in Images. Uncertainty stays internal rather than becoming a
  browsing category. The Inspector's **Photography** switch and image card actions let the user
  correct inclusion in Photography or return to automatic classification. Corrections live in
  the reading's file and take precedence over later model results. Classification does not create
  a Tag or a card kind.
  Automatic classification uses the on-device Apple Intelligence image model on macOS 27 or
  later. When that model is unavailable, images remain pending and manual classification works;
  returning to the app retries after the model becomes available.
- The selected board scope, free-text query, and every completed search token compose as an
  intersection. Filtering is performed in the Rust core, not on a Swift-side subset, so the
  complete board snapshot remains correct. Free text searches reading text, user and machine Tags,
  and derived visual terms. Exact Tag tokens match the combined Tags shown in the Inspector;
  visual terms remain search facts rather than separate Tag suggestions. Board order is fixed:
  newest saved first when browsing and relevance when searching.
- `⌘F` focuses the native search field; `/` does the same while the board has keyboard focus.
  On the board, `⌘1`–`⌘6` select All through Quotes in toolbar order, and `⌘[` / `⌘]` cycle
  the scopes. Detail uses `⌘1` and `⌘2` for its preview modes instead.

### Masonry cards

- A true masonry layout uses equal-width columns with variable-height cards and **20–24 pt** gaps.
  At a wide desktop window it should naturally reach four or five columns around 220–250 pt each.
- **Image:** local captured asset, full bleed, preserving a convincing square/portrait/landscape
  mix. Board images and article previews taller than 2.5 times their width are top-aligned
  and clipped to 1.25 times their width, with a bottom gradient fading into the board surface
  to indicate that the image continues; shorter images remain complete. Detail shows the full
  image. Redundant title chrome is hidden.
- **Video:** captured poster image with a restrained play glyph. A durable media URL is secondary
  metadata; session-local streams fall back to the source page for playback.
- **Quote:** selected text rendered in centered Georgia Italic, using the article description's
  secondary text color, with 20 pt padding all around and muted quotation marks. Its origin remains
  available in detail and the Inspector, but is omitted from the board card. Previews longer than
  240 characters use one smaller type size; previews longer than 480 use two, with a 16 pt minimum.
  Line spacing, quotation marks, and their spacing scale with the text so longer quotes show more
  within 12 lines.
- **Featured highlight:** an article card led by a saved passage in larger, left-aligned system
  text, with yellow marker ink behind each wrapped line and no quotation marks or source footer.
  The source article and all saved highlights remain available in detail. Saving another ordinary
  selection from the same article changes its featured passage without creating another card.
- **Article:** a local preview image when available, followed by a Palatino heading, saved or
  file-derived description, source, and estimated reading time. Text-only article cards fit their
  visible content with 20 pt padding; source and reading time are muted, and absent favicons leave
  no decorative placeholder. Lightweight links keep their separate card treatment.
- **Social post:** a source-aware article card with avatar, author/handle, full post text, provider,
  and the first local attachment or video poster. It appears in Links while retaining its full local
  article file and native post detail. Its source profile does not itself create a Tag; the saved
  post text and attachments can acquire subject Tags through the same analysis as other readings.
- **Product:** an image-led link card with the saved price in a top-right badge using semantic
  light/dark colors. A larger title overlays the bottom of the local preview over a dark gradient;
  there is no source footer. Previews fit without cropping inside a square or gently portrait
  canvas. Products without previews use a neutral surface and semantic title color. Minimum offer
  prices say **From**; a recognised product without a usable price says **Product**. Product
  classification is file-backed metadata, creates no Tag, and stays in Links, including full
  browser captures.
- **Book:** a portrait cover with a subtle shaded spine, using the local artwork's proportions
  when suitable. **Music:** a square album cover with a vinyl record peeking above it.
  **Movie:** a portrait poster with transparent filmstrip perforations down each side. These
  artwork cards have no source footer or title overlay. Without artwork they show a semantic
  category symbol and a readable title. Their category stays in file-backed metadata and Links;
  a more specific Book/Music/Movie category takes precedence over Product styling.
- Textual cards (articles, lightweight links, quotes, and social posts other than 𝕏 posts) use a
  neutral surface that reads slightly lighter than the board in both appearances. 𝕏 post cards
  use a subtle cool-gray surface in light mode and a slightly lighter cool gray than the board in
  dark mode, with semantic text and symbol colors. The board and window shell use
  progressively darker shades of the same semantic background. Image and video cards retain their
  material-backed media treatment.
- Cards have 8–12 pt continuous corners and a semantic separator border, with no decorative lift
  or shadow effects. Actions live in the context menu and do not clutter the board.
- A single click selects and focuses a card without opening it. Selection uses a restrained semantic
  accent outline. Arrow keys move spatially through the masonry columns and minimally scroll the
  focused card into view; Shift-click and Shift-arrow extend the selection from its anchor.
  Command-click toggles an individual card; Command-Shift-click adds a range to the existing
  selection. `⌘A` selects every card in the current board scope and search results.
- Double-click or Return opens the focused card. `⌘O` opens its source in the system browser.
  Space opens system Quick Look for its local
  media asset, falling back to the source-of-truth `article.md`; Quick Look never receives a remote
  origin URL. Pinching changes the same five card-size levels as the toolbar and existing keyboard
  zoom commands. On the board, `⌘-` decreases card size and `⌘+` increases it; `⌘=` also increases
  card size. These commands do not change card size from detail.
- The card context menu's **Add Tag…** action and `⌘T` open an **Add tags** dialog for the
  selected cards. The board toolbar has no tag control.
  Its inline field uses the same library Tag suggestions as the Inspector; clicking a suggestion
  completes the field. The field receives initial keyboard focus. Suggestions float below the
  field without changing the dialog’s size. The dialog fits its Tags; long Tag collections scroll.
  Return adds one user Tag to every target, preserving each card's existing Tags and avoiding duplicates.
  `⌘T` remains available while editing text and can reopen the dialog after dismissal.
  The dialog retains its target cards while background refreshes run and remains open for
  additional names until the top-right close button, Escape, or a backdrop click. The padded
  dialog has a subtle border, appears over a dark backdrop, and fades in and out without movement.
  With one card selected, it shows every user and
  machine Tag. With several selected, it shows the names added during that dialog session.
  These Tags use the Inspector's same pills, hover removal control, label search, and machine
  ownership action. Removal from this dialog requires confirmation. Edits appear immediately;
  ordered background writes and authoritative row reloads reconcile their stored state, including
  cards hidden by a tag filter.
- Delete applies to every selected card after one explicit confirmation. Commands that only make
  sense for one reading, such as opening its source, are unavailable while several cards are
  selected. `⌘Delete` requests the same confirmed permanent deletion.
- Every card kind exposes its **origin page** (page URL, canonical URL, title/site, saved date).
  Image/video `media_url` is secondary metadata and never replaces the origin.
- In grid and detail, `⌘C` copies the selected reading's local content: the image, video, or
  saved Markdown as appropriate. `⇧⌘C` copies its source URL. Native editable text fields keep their
  standard `⌘C` behavior.

### Context menu and capture

- Browser right-click uses one **"Save to Óia"** command for a page, image, video, or selected
  text. A selection becomes an article highlight by default; matching enclosing quotation marks or
  a selection wholly within a `<blockquote>`/`<q>` element instead creates a quote card.
  Image bytes and video posters are copied locally when
  available.
- The native card context menu provides tag editing, open origin, and permanent delete. On the
  board, **Add Tag…** opens the dialog for the complete selection when the clicked card belongs
  to it, or just that card otherwise. In Gallery, tag editing focuses the Inspector's inline field.
  Permanent card deletion retains confirmation.

### Card detail

- Double-click or Return opens a full-window Gallery detail for articles, images, videos,
  quotes, and captured social posts while preserving the board behind the navigation destination.
  A lightweight link opens its origin in the system browser instead and never enters Gallery.
  Opening collapses a multi-selection to that card. Escape or Close returns focus to the board;
  left/right and J/K move through the frozen detail-capable board order, skipping lightweight links.
  In detail, `⌘O` opens the source in the system browser and `⌘T` focuses the Inspector's tag
  field, showing the Inspector when needed.
- The selected preview fills the available space, extending behind the transparent toolbar for
  saved articles, Original pages, and images. Collapse, Previous, Next, and Close sit on the
  left in that order. Previous and Next move through the frozen detail-capable board order. `⌘B`
  toggles the leading inspector sidebar with a brief slide, or immediately when Reduce Motion is enabled.
- Ordinary articles reuse the existing native Markdown reader. Social-post articles use a native
  post detail with selectable text and every ordered local attachment. Articles with a web origin
  offer **Saved** and **Original** in the toolbar. Saved is the initial default; choosing either
  view stores a per-device preference, retained across navigation and reopening detail. Articles
  without a web origin show Saved without changing the preference. Original loads the live origin
  in an embedded WebView when selected. Switching preserves the saved reader and original page's
  position. `⌘1` selects Saved and `⌘2` selects Original, including while the original page has
  focus. Detail views show no loading indicators, labels, animated backgrounds, or extra toolbar
  status items while article content or local media arrives. Failed loads offer Try Again and
  Open in Browser; Saved
  remains available offline. In-page links scroll Original; links to other pages open in the system
  browser. Images offer **Fit** and **Original Size** in the toolbar. Fit shows the whole local asset
  within the window; Original Size displays one source pixel per display pixel, with horizontal
  and vertical scrolling. Images use `⌘1` for Fit and `⌘2` for Original Size. Images taller
  than 2.5 times their width initially open at Original Size, aligned to the top; other images
  initially open in Fit. Choosing either size saves a per-device preference, retained across
  previous/next navigation and reopening detail. Images that fit stay centered in both modes;
  switching modes preserves the loaded image, resetting scroll only when its displayed size changes.
  Videos show the local poster and source/media actions without silently downloading a stream.
  Quotes show the full selected text natively. Short quotes use centered 34 pt Georgia Italic.
  Quotes longer than 480 visible characters or containing multiple paragraphs use leading-aligned
  24 pt Georgia Italic in a 680 pt measure; leading, quotation marks, and surrounding spacing scale
  together with the smaller text.
- Metadata lives in the full-height leading Inspector, shown by default and toggled from the
  titlebar. Its visibility is a per-device preference. The sidebar scrolls within short windows.
  Delete sits below the content and requires confirmation. macOS 15 uses system material, and
  Reduce Transparency uses an opaque system surface.
- Below the title, the Inspector orders its sections as **Tags**, **Colours**, **Details**, then
  Delete. The Tags section shows user and automatically generated subject Tags together, with
  neutral Liquid Glass styling. User Tags show the SF Symbol `tag`; machine Tags show `sparkles`.
  Both icons have the same spacing from the label. Hovering a Tag changes its icon to
  `xmark.circle.fill`; clicking it removes the Tag from this reading immediately. The label
  searches the library. An inline field below the Tags heading filters a dropdown of matching
  library Tags, like board search.
  Choosing a suggestion fills the field; Return adds the entered name as a user Tag. Keyboard and
  context-menu tag-editing actions in Gallery focus this field. Adding a user Tag with
  the same spelling ignoring case gives the user Tag ownership in the combined view. Removing any
  effective Tag removes its user label and excludes matching machine labels, including future
  results. Analysis adds machine Tags directly after
  saving or processing a reading, without a review step. There is no separate image-label section
  or image-label suggestion picker. Removing a Tag from a reading does not delete that name globally.
  **Colours** shows up to five distinct swatches for media and links as closely spaced
  flat circles with a subtle outline; full articles omit it. Swatch fills stay colour-accurate;
  hovering strengthens only the outline. Selecting one starts `colour:#RRGGBB` search, ranked by
  perceptual shade similarity. Cached swatches are part of the board snapshot so the section
  appears at its final height when opening a card; readings without colours omit it immediately.
  The inspector still checks the local asset in the background and discards stale swatches.
- **Details** always shows Finder-style labeled rows using regular secondary labels and regular
  primary values. Tags, Colours, and Details share the same 12 pt semibold section-heading style.
  The rows show the save date and, when available, reading time, local media duration,
  dimensions, codecs, color profile, format, size, author,
  and origin. Media facts come from the local file and unavailable facts are omitted. Article
  preview file facts are labeled as previews. Internal asset schemes and hashed filenames never
  appear. The source appears once as a hostname followed by ↗; its complete URL is available on
  hover or through Copy source URL. Analysis and missing-file states never prevent access to
  source or tags.

### Content states

- **Empty state:** quiet browser-extension guidance for saving a page, media item, or selection.
- **Board:** the complete masonry result for the active board scope and search query. Card views are
  materialized only around the visible viewport; the board has no pagination control or loading row.
- **No results:** identifies the active search/filter and offers to clear that axis.

### Settings / appearance

- **Appearance:** segmented **Light / Dark / System**.
- **Font:** family picker (default **Inter**) and a size **slider** (small → large "Aa").
- These are **per-device UI preferences** — stored in app preferences (e.g. `UserDefaults`),
  **not** in the library folder and **not synced**, consistent with the "the DB/cache is
  per-device" principle in [AGENTS.md](./AGENTS.md).
- These controls live in the standard macOS **Settings** scene, opened from the application menu
  or ⌘,. This surface also contains the **library-folder path** and **native-host status**.

## Apple platform style guide — the reader

> This section is **exclusive to the Apple (macOS) client** and is the source of truth for how a
> saved article is rendered in the reader. The visual reference is NetNewsWire’s Biblioteca
> theme: system serif prose and title, sans-serif section headings, compact paragraph rhythm,
> and subtle quote rules. Native selection, adjustable typography, and semantic Light/Dark colors
> preserve the macOS reading experience.
>
> It is implemented natively with SwiftUI + [`swift-markdown`](https://github.com/apple/swift-markdown)
> and AppKit text views (**no WebView**). Type sizes and most spacing follow the chosen body size;
> the reading measure is an independent width preference.

### Where it lives

| Concern | File |
|---------|------|
| Font and spacing tokens | `macos/Sources/Oia/Features/Reader/Markdown/MarkdownTheme.swift` |
| Continuous text, headings, lists, and quotes | `macos/Sources/Oia/Features/Reader/Markdown/MarkdownTextRun.swift` and `AppKitInline.swift` |
| Native text selection and layout | `macos/Sources/Oia/Features/Reader/Markdown/SelectableTextView.swift` and `ReaderTextView.swift` |
| Images, code, tables, and image-bearing blocks | `macos/Sources/Oia/Features/Reader/Markdown/MarkdownBlockView.swift`, `InlineRenderer.swift`, and `AssetImageView.swift` |
| Parsing, scroll container, and reading measure | `macos/Sources/Oia/Features/Reader/Markdown/MarkdownDocumentView.swift` |
| Article title and metadata | `macos/Sources/Oia/Features/Reader/Article/ArticleHeaderView.swift` and `ArticleDetailView.swift` |

### Reader typography

**Serif** is the default reading face, using the platform serif (`ui-serif` in Biblioteca).
Section headings H2–H6 use the system sans-serif. **System** and **Monospace** body choices
remain available in Settings › Typography. All preferences are per-device and do not alter files.
See [the theme reference](docs/reader-typography.md).

| Size option | Body point size |
|-------------|-----------------|
| Small | 16 pt |
| **Medium (default)** | **18 pt** |
| Large | 20 pt |
| Extra Large | 22 pt |
| Huge | 24 pt |
| Giant | 26 pt |

The left-aligned article title and body use the same measure. Increasing text size does not silently
widen the column.

| Width | Measure |
|-------|---------|
| Extra Small | 520 pt |
| Small | 600 pt |
| **Medium (default)** | **680 pt** |
| Large | 800 pt |
| Extra Large | 960 pt |

Normal line height is **1.6×** the body size. Paragraphs have **1em** of separation. List items remain closely grouped, and headings sit closer
to the paragraph they introduce than to the preceding text. The selectable text run and the
SwiftUI blocks use the same spacing tokens so an image or code block does not change the rhythm.

**Line height** and **content width** remain user-adjustable (`ReaderLineHeight`, `ReaderWidth`).
Normal is the middle of five line-height stops; the other stops tighten or loosen leading without
changing the font size. Width also has five stops, with Medium as the default.

| Line height | Effective |
|-------------|-----------|
| Tight | 1.20× |
| Snug | 1.475× |
| **Normal (default)** | **1.60×** |
| Relaxed | 1.75× |
| Loose | 2.00× |

### Heading hierarchy

Biblioteca uses a bold serif title/H1 and bold sans-serif H2–H6, all with 1.2 line height.
Headings have 1.5em above and 0.25em below, relative to their own size. A following paragraph
has no added top gap, so the heading remains connected to its content. Tags are not shown
under the article title.

| Level | Scale | Treatment |
|-------|-------|-----------|
| Article title / H1 | `2em` | bold system serif, left aligned |
| H2 | `1.5em` | bold system sans-serif |
| H3 | `1.17em` | bold system sans-serif |
| H4 | `1em` | bold system sans-serif |
| H5 | `0.83em` | bold system sans-serif |
| H6 | `0.67em` | bold system sans-serif, separate block |

The article title comes from frontmatter and is the reading's primary heading. Browser captures
demote a body `#` to `##` so the same title does not appear twice. Older or manually written files
may still contain body H1s; they retain heading styling when parsed. H6 remains a separate block in both renderers.

### Lists

| Aspect | Spec |
|--------|------|
| Gap between items | `0.5em`, less than the `1em` gap between paragraphs |
| Marker → text gap | `0.4em`, with wrapped lines aligned to the item text |
| Marker column | hanging indent; `1.5em` ordered / `1.1em` unordered, with ordered numbers aligned neatly |
| Unordered bullets | the same quiet, neutral bullet at each depth; indentation shows nesting |
| Ordered markers | `1.`, `2.`, … in a neutral color |
| Task lists (GFM) | outlined, neutral checked and unchecked squares; the checked state is legible without an accent fill |
| Nesting | each level steps in while preserving the compact item rhythm |
| Rich item content | a list item may contain multiple paragraphs, code, quotes, or nested lists |

### Block quotes

- Use the ordinary upright body face and size, with **15 pt** of left padding plus a
  subtle **3 pt** rule. Quotes retain normal paragraph spacing and can nest.
- Continuous native text selection remains available across paragraphs and text-only quotes.

### Code

| Kind | Spec |
|------|------|
| Inline code | monospaced, close to the body size, with restrained neutral emphasis |
| Code block | monospaced on a flat, lightly separated surface; no rounded card treatment; horizontal scroll for long lines |
| Language label | retained in the Markdown source but not displayed in the reader |

> **Not yet:** syntax highlighting — code is rendered as uniform monospaced text.

### Tables (GFM)

- Rendered with a SwiftUI `Grid`. Thin horizontal rules sit above the header, below the header, and
  below the table; there are no vertical dividers. Header labels are uppercase and bold.
- **Column alignment is honored** — left / center / right per the table's `:---`, `:--:`, `---:`.
- Keep cell padding modest; body cells support inline styling and text selection.

### Images & figures

- Scaled to fit the content width, **corner radius 6**; centered.
- **Alt text becomes a caption** rendered below the image (centered, `0.85em`, secondary) — i.e. a
  proper *figure*. Images with empty alt show no caption.
- Local library assets load from the reading's own `assets/` folder
  (`articles/<prefix>/<id>/assets/<file>`), linked from the body as `assets/<file>`. An image the extension couldn't capture at
  save time stays a remote `http(s)` URL; the reader shows a labelled placeholder for it and never
  fetches it over the network.
- The common `[![alt](img)](url)` pattern (a link wrapping a single image) is unwrapped and rendered
  as the image.

### Inline text styles

| Element | Rendering |
|---------|-----------|
| **Bold** (`**`/`__`) | bold run in the chosen reading face |
| *Italic* (`*`/`_`) | italic run |
| ~~Strikethrough~~ (GFM) | strikethrough line |
| `Inline code` | monospaced and neutral (see Code) |
| [Links](#) | neutral text with an underline; open in the **system browser** |
| Nested emphasis | composes correctly (e.g. bold-inside-italic-inside-a-link) |
| Hard / soft line breaks | preserved / collapsed to a space |

### Other blocks

- **Thematic break** (`---`): a thin, neutral horizontal rule with breathing room above and below.
- **Raw HTML**: block tags are stripped to visible text in a secondary color; inline HTML tags are
  omitted. Raw markup is never rendered or executed.

### Article header chrome

At the top of the scrolling reader, each article shows:

- **Title** at `1.5em` in the selected reader face. It shares the body's width and grows with the
  chosen body size.
- **Tags** appear as a read-only text summary of user and machine Tags. The **Add Tag…** command
  focuses the Inspector's inline Tag field, so managing tags does not reflow the article header.
- The header has enough space below it to start the body clearly, without a decorative divider.

The leading inspector is a full-height, borderless sidebar with a translucent, blurred macOS
sidebar material. Reduce Transparency uses a solid surface instead. It shows
estimated reading time for full articles, in a quiet secondary style beside the source and save date.
Its word count is available on hover. Article bodies show links in neutral underlined text in both
the selectable text run and image-bearing blocks.

### Text selection

The reader supports **continuous, native selection** (drag, double/triple-click, context-menu copy) across
contiguous text. SwiftUI's `Text` + `.textSelection` only selects inside one `Text`, so headings,
paragraphs, and **image-free lists and quotes** are coalesced into a single `NSAttributedString`
rendered by a read-only `NSTextView` (`SelectableTextView`). The theme's spacing, list hanging
indents, and quote indents are expressed as `NSParagraphStyle` attributes. This is AppKit/TextKit,
not WebKit.

- A run breaks at figures, code blocks, tables, thematic breaks, and image-bearing lists or quotes.
  These render as native SwiftUI blocks and form selection seams.
- Run height is driven by the text view's `intrinsicContentSize` (invalidated whenever the text or
  width changes), keeping long articles visible while the type settings change.
- ⌘F find-bar isn't offered per run (a standalone `NSTextView` needs an enclosing scroll view for
  it). Selected text can be copied from the context menu; `⌘C` copies the reading's local file.

### Element coverage & known limitations

The renderer covers the full CommonMark + GFM surface that the extension's HTML→Markdown step can
produce. Anything unrecognized recurses into its children so **no content is silently dropped**.

| Element | Supported | Notes |
|---------|-----------|-------|
| Headings H1–H6 | ✅ | full six-level hierarchy (above) |
| Paragraphs, emphasis, strong, strikethrough, inline code | ✅ | |
| Links, line/soft breaks | ✅ | links open in the system browser |
| Images / figures with captions | ✅ | local assets; missing images show a placeholder |
| Ordered / unordered / nested lists | ✅ | depth-aware bullets & indent |
| Task lists (checkboxes) | ✅ | GFM |
| Block quotes (incl. nested) | ✅ | |
| Code blocks (fenced or indented) | ✅ | language metadata is kept in Markdown; no syntax highlighting yet |
| Tables with column alignment | ✅ | GFM |
| Thematic break / horizontal rule | ✅ | |
| Raw HTML (block & inline) | ⚠️ | block tags are stripped to visible text; inline tags are omitted |
| Footnotes (`[^1]`) | ❌ | swift-markdown doesn't model them; render as literal text |
| Math / LaTeX | ❌ | not rendered |
| Definition lists, sub/superscript | ❌ | not in CommonMark; would arrive as HTML and be stripped |

> When the upstream extension produces these unsupported constructs, prefer normalizing them during
> HTML→Markdown (e.g. flatten footnotes, drop math) so the reader stays clean. Revisit footnotes and
> syntax highlighting as future enhancements.

## Organization & data-model impact

The visible organizing model is deliberately small:

| Curation | Frontmatter field | Notes |
|----------|-------------------|-------|
| Tags | `tags`, `machine_tags`, `excluded_machine_tags`; see [library format](./docs/library-format.md) | user labels, machine labels with source fingerprints and analyser versions, and excluded label keys remain in the article file; the effective union is indexed by search |

The main board includes every saved item. The format-v1 `read_at`, `archived`, `favorite`, and
`rating` fields remain readable and round-trippable for compatibility with existing libraries and
older clients, but the current macOS app neither displays nor mutates them.

## Interaction model — optimistic & self-healing

Tag changes **apply to the UI instantly**; persistence happens in the background. The user clicks,
the tag flips on the next frame, and the core write + index refresh run behind it. Because the
Markdown file is the source of truth and the refresh re-reads from it, a failed write simply
reconciles back — no spinners, no manual undo.

Automatic analysis runs after the saved reading and its assets are durable, or after a scan finds
new or changed files. Its completion revalidates the reading against the analysed source before
writing machine Tags to frontmatter and refreshing the index. Removing any effective Tag records
an exclusion in that same file and removes a matching user label, so analysis does not restore the
Tag on a later pass. The removal applies only to that reading. User Tags take precedence over
case-insensitive machine matches.

**One motion, not two.** When removing a tag moves a card out of the active tag filter, the row
slides out **and** selection advances to the neighbouring card in the same beat.
The reader follows to the next item. (Flipping the icon in place and letting the row jump a moment
later, on the async refresh, reads as a stutter; this avoids it.) Row *re-ordering* after an edit
still settles on the background refresh — only removal/advance is immediate.

When the open card leaves the active scope, the detail overlay advances with the board to that same
neighbour (or closes when no card remains). A post-edit refresh then reconciles ordering without a
second selection jump. Direct reloads reconcile against the matching cards without inventing a new
selection; the first load begins unselected.

## Paste and drop

- The whole card board is a drop target for HTTP(S) links, text, and images, including its empty
  state. While a supported item is over the window, a clear non-blocking overlay says it can be
  dropped to save.
- With no text field active, standard Paste (⌘V) saves the same payloads through the same path.
  Search, tag, and other text editors retain normal paste behavior.
- Plain text becomes a quote card. Image data and supported local image files are copied into the
  reading's `assets/` folder. Local text/Markdown files use their file name as the card title.
- A pasted or dropped URL uses the shared Rust URL-save facade. A strictly recognized public source
  becomes a full local article; every other URL remains a marked lightweight article card rather
  than fake extracted content. The browser extension remains the full-fidelity path for ordinary,
  authenticated, and JavaScript-rendered pages and upgrades a matching placeholder in place.
- Unsupported payloads such as PDFs, generic files, and local video files are rejected with visible
  feedback; the app does not create cards it cannot render faithfully.

## Deferred / out of scope (for now)

- **Generic core-side page cleanup.** Source adapters are intentionally narrow; the app does not
  reproduce the extension's live-DOM extraction. Authenticated and JavaScript-rendered pages
  continue to require the browser extension.
- **Lists.** The mockup's "Lists" section is replaced by **Tags**; manual Lists are not planned.

## Open questions / to finalize

- ~~Exact extension color tokens and brand accent~~ — **resolved:** see the
  [Brand color palette](#brand-color-palette); the macOS app uses semantic system colors.
- Exactly which quiet source metadata belongs on each card kind as the library grows.
