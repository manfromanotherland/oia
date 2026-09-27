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
  *"Search Óia"*. Native token suggestions can narrow the board to exact tags or terms found in
  the same image; completed terms remain value-only pills in that one field. Do not create a
  bespoke `NSSearchField`, duplicate search control, or oversized page header.
- Selecting a completion under **Tags** or **In this image** creates that scoped token. Pressing
  Return without selecting a completion keeps the draft as broad free text; never infer an exact
  tag merely because a tag has the same spelling.
- Use one native labeled segmented picker for the board scope, in this order: **All, Media,
  Articles, Links, Quotes**. Media combines image and video cards; Articles excludes lightweight
  link placeholders; Links means lightweight URL saves. Exact tag filtering comes from native
  suggestions in the toolbar search field.
- The selected board scope, free-text query, and every completed search token compose as an
  intersection. Filtering is performed in the Rust core, not on a Swift-side subset, so the
  complete board snapshot remains correct. Multiple visual terms must occur in the same reading's
  visual analysis; unrelated title, body, and tag text cannot satisfy them. Board order is fixed:
  newest saved first when browsing and relevance when searching.
- `⌘F` focuses the native search field; `/` does the same while the board has keyboard focus.
  `⌘1`–`⌘5` select All through Quotes in toolbar order, and `⌘[` / `⌘]` cycle the scopes.

### Masonry cards

- A true masonry layout uses equal-width columns with variable-height cards and **20–24 pt** gaps.
  At a wide desktop window it should naturally reach four or five columns around 220–250 pt each.
- **Image:** local captured asset, full bleed, preserving a convincing square/portrait/landscape
  mix. Redundant title chrome is hidden.
- **Video:** captured poster image with a restrained play glyph. A durable media URL is secondary
  metadata; session-local streams fall back to the source page for playback.
- **Quote:** selected text rendered as a left-aligned editorial typographic card. Its origin remains
  available in detail and the Inspector, but is omitted from the board card.
- **Article:** first local image plus compact title/domain treatment, or a text-led card when no
  preview exists.
- **Social post:** a source-aware article card with avatar, author/handle, full post text, provider,
  and the first local attachment or video poster. It remains in Articles and does not acquire an
  automatic tag.
- Cards have 8–12 pt continuous corners and a semantic separator border. Hover reveals only a
  small standard action menu; cards do not add decorative lift or shadow effects. Tags and actions
  do not permanently clutter the board.
- A single click selects and focuses a card without opening it. Selection uses a restrained semantic
  accent outline. Arrow keys move spatially through the masonry columns and minimally scroll the
  focused card into view; Shift-click and Shift-arrow extend the selection from its anchor.
- Double-click, `⌘O`, or Return opens the focused card. Space opens system Quick Look for its local
  media asset, falling back to the source-of-truth `article.md`; Quick Look never receives a remote
  origin URL. Pinching changes the same five card-size levels as the toolbar and existing keyboard
  zoom commands.
- Delete applies to every selected card after one explicit confirmation. Commands that only make
  sense for one reading, such as editing tags or opening its source, are unavailable while several
  cards are selected. `⌘Delete` requests the same confirmed permanent deletion.
- Every card kind exposes its **origin page** (page URL, canonical URL, title/site, saved date).
  Image/video `media_url` is secondary metadata and never replaces the origin.

### Context menu and capture

- Browser right-click uses one **"Save to Óia"** command for a page, image, video, or selected
  text. Selection becomes a quote card; image bytes and video posters are copied locally when
  available.
- The native card context menu provides tags, open origin, and permanent delete.
  Destructive actions retain confirmation.

### Card detail

- Double-click, `⌘O`, or Return opens a full-window Gallery detail for articles, images, videos,
  and quotes while preserving the board behind the navigation destination. A lightweight link
  opens its origin in the system browser instead and never enters Gallery. Opening collapses a
  multi-selection to that card. Escape or Back returns focus to the board; left/right and J/K move
  through the frozen detail-capable board order, skipping links.
- The selected preview fills the available space above a persistent horizontal filmstrip. Clicking
  a filmstrip thumbnail selects it, and keyboard navigation keeps the selected thumbnail visible.
- Ordinary articles reuse the existing native Markdown reader. Social-post articles use a native
  post detail with selectable text and every ordered local attachment. Images show the local asset aspect-fit.
  Videos show the local poster and source/media actions without silently downloading a stream.
  Quotes show the full selected text natively.
- Metadata lives in a floating trailing Inspector, shown by default and toggled from the standard
  toolbar. Its visibility is a per-device preference. A 320 pt panel uses a 20 pt continuous corner
  radius, 24 pt exterior space, and a content-height native Liquid Glass surface. It scrolls within
  short windows; the filmstrip spans the full window below it. macOS 15 uses system material, and
  Reduce Transparency uses an opaque system surface.
- **Discover** separates **Your tags** from **In this image**, with a small adjacent information
  button explaining locally recognised suggestions. Tags and attributes are native capsule glass
  buttons. Attributes search the library; editing tags uses the existing optimistic tag picker.
  Up to five distinct colours appear as closely spaced flat circles with a subtle outline. Their
  fills stay colour-accurate; hovering strengthens only the outline. Selecting one starts
  `colour:#RRGGBB` search, ranked by perceptual shade similarity.
- **Details** shows friendly local format, display-oriented dimensions, size and save date. Internal
  asset schemes and hashed filenames never appear. The source appears once as a hostname followed
  by ↗; its complete URL is available on hover or through Copy source URL. Analysis and missing-file
  states never prevent access to source or tags. Tags, origin and delete remain available in the toolbar.

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
> saved article is rendered in the reader. It follows the
> [Apple Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/typography):
> a clear typographic hierarchy, a comfortable reading measure, asymmetric "section" spacing, and
> native controls (text selection, Dynamic-Type-like sizing, semantic Light/Dark colors).
>
> It is implemented natively with SwiftUI + [`swift-markdown`](https://github.com/apple/swift-markdown)
> (no WebView). Every value below is expressed **relative to the reader's body size** so the whole
> document rescales when the user changes the text size.

### Where it lives

| Concern | File |
|---------|------|
| All fonts / sizes / weights / spacing tokens | `Sources/Oia/Views/Markdown/MarkdownTheme.swift` |
| Inline runs → styled `AttributedString` (bold, italic, code, links…) | `…/Markdown/InlineRenderer.swift` |
| Block rendering (headings, lists, quotes, tables, code…) | `…/Markdown/MarkdownBlockView.swift` |
| Images / figures + captions | `…/Markdown/AssetImageView.swift` |
| Scroll container, reading measure, link handling | `…/Markdown/MarkdownDocumentView.swift` |
| Article header chrome (title, metadata, tags) | `Sources/Oia/Views/ArticleDetailView.swift` |

### Reader typography

The body font is user-selectable (**System** = San Francisco, **Serif** = New York, **Monospace** =
SF Mono) at four sizes — these are the per-device preferences described under *Settings / appearance*.

| Size option | Body point size |
|-------------|-----------------|
| Small | 15 pt |
| **Medium (default)** | **17 pt** |
| Large | 19 pt |
| Extra Large | 21 pt |

Everything else is a multiple of this body size, so the document keeps its proportions at any size:

| Token | Value (× body) | At 17 pt | Role |
|-------|----------------|----------|------|
| Line spacing | `0.55em` | ~9.4 pt | added leading → effective line-height ≈ **1.75** (Normal) |
| Block spacing | `1.0em` | 17 pt | vertical gap between top-level blocks |
| Content measure | `680 pt` (Medium) | — | optimal line length (~60–75 chars); content is centered |

**Line height** and **content measure** are user-adjustable in Settings › Typography
(`ReaderLineHeight`, `ReaderWidth`). The values above are the defaults, and each is the
**middle stop** of its five, so the middle choice lands on the default.

| Line height | Effective | Added leading (× body) |
|-------------|-----------|------------------------|
| Tight | 1.25 | `0.05em` |
| Snug | 1.50 | `0.30em` |
| **Normal (default)** | **1.75** | **`0.55em`** |
| Relaxed | 2.00 | `0.80em` |
| Loose | 2.25 | `1.05em` |

| Width | Measure |
|-------|---------|
| Extra Small | 520 pt |
| Small | 600 pt |
| **Medium (default)** | **680 pt** |
| Large | 800 pt |
| Extra Large | 960 pt |

The measure is a fixed point value, not a multiple of the body size, so bumping the
text size doesn't silently widen the column too. The article header shares it, so the
title stays flush with the body at every width.

### Heading hierarchy

The old reader left headings looking like body text — every inline run carried the body font, which
overrode the heading font. The renderer now injects a per-heading **font context** so all six levels
are visually distinct. Headings get more space **above** than below, so a heading visually *binds to
the section it introduces* (a core HIG/typography principle) and major sections get a clear break.

| Level | Size (× body) | At 17 pt | Weight | Tracking | Space above (× body) | Treatment |
|-------|---------------|----------|--------|----------|----------------------|-----------|
| **H1** | 1.80 | ~30.6 pt | Bold | −0.5 | 1.7em | page/section title |
| **H2** | 1.45 | ~24.7 pt | Bold | −0.3 | 1.3em | major section |
| **H3** | 1.20 | ~20.4 pt | Semibold | 0 | 1.0em | sub-section |
| **H4** | 1.05 | ~17.9 pt | Semibold | 0 | 0.8em | minor heading |
| **H5** | 0.95 | ~16.2 pt | Semibold | 0 | 0.6em | small heading |
| **H6** | 0.85 | ~14.5 pt | Semibold | +0.6 | 0.6em | **uppercase eyebrow**, secondary color |

- **Tracking:** large display headings are tightened (Apple tightens large titles); the small H6
  eyebrow is opened up and uppercased for legibility.
- `Space above` is **added on top of** the 1em block spacing, so the gap above a heading is always
  larger than the gap below it.
- **The article title is the sole H1.** The header draws the title from the frontmatter `title` at
  the H1 size; the extension demotes any body `#` to `##` (see library-format), so body headings
  start at H2 and never duplicate the title.

### Lists

| Aspect | Spec |
|--------|------|
| Gap between items | `0.5em` (tighter than the 1em block gap) |
| Marker → text gap | `0.5em` |
| Marker column | fixed hanging indent — `1.5em` (ordered) / `1.1em` (unordered/task), right-aligned |
| Unordered bullets | cycle by nesting depth: **`•` → `◦` → `▪`**, in secondary color |
| Ordered markers | `1.`, `2.`, … with **monospaced digits**, secondary color |
| Task lists (GFM) | `☑` (`checkmark.square.fill`, **accent**) / `☐` (`square`, secondary) |
| Nesting | nested lists indent one marker column and step the bullet/number down a level |
| Rich item content | a list item may contain multiple paragraphs, code, quotes, or nested lists |

### Block quotes

- A **3 pt rounded vertical bar** (secondary, 40% opacity) with a `0.85em` gap to the quoted content.
- Quoted text is rendered in the **secondary** color; inner blocks use `0.6em` spacing.
- Quotes may nest and may contain any block (paragraphs, lists, code, even other quotes).

### Code

| Kind | Spec |
|------|------|
| Inline code | monospaced at `0.9em`, subtle `secondary @ 15%` background |
| Code block | monospaced at `0.9em` on a `secondary @ 10%` surface, **corner radius 8**, padding 14, **horizontal scroll** (no wrapping) |
| Language label | when a fence declares a language it's shown lowercased above the block in the caption style |

> **Not yet:** syntax highlighting — code is rendered as uniform monospaced text.

### Tables (GFM)

- Rendered with a SwiftUI `Grid`; **header row is semibold** with a divider beneath it.
- **Column alignment is honored** — left / center / right per the table's `:---`, `:--:`, `---:`.
- Horizontal spacing 16 pt, vertical 8 pt; cells support full inline styling and text selection.

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
| **Bold** (`**`/`__`) | semibold/bold run |
| *Italic* (`*`/`_`) | italic run |
| ~~Strikethrough~~ (GFM) | strikethrough line |
| `Inline code` | monospaced + tinted background (see Code) |
| [Links](#) | **accent** color + underline; open in the **system browser** |
| Nested emphasis | composes correctly (e.g. bold-inside-italic-inside-a-link) |
| Hard / soft line breaks | preserved / collapsed to a space |

### Other blocks

- **Thematic break** (`---`): a full-width divider with `0.5em` vertical breathing room.
- **Raw HTML** (block or inline): **tags are stripped**; only the visible text is shown (secondary
  color). Raw markup is never rendered or executed.

### Article header chrome

Above the scrolling reader (in `ArticleDetailView`), each article shows:

- **Title** at the **H1 type token** (bold) — the reading's sole h1. Like the metadata,
  it is sized from the reader's body size, so the whole header rescales when the reader changes the
  font size instead of staying fixed while the copy grows.
- **Metadata row** — site (`globe`), author (`person`), and estimated article length (`clock`) as
  secondary labels sized from the body, shown only when present. Length is derived from word
  count at an average **200 wpm**, rounded up to a 1-minute minimum (e.g. "5 min"); the
  raw word count is kept as the label's hover tooltip. Icon–label gaps use the 4 pt
  icon–label spacing token.
- **Tags** as rounded **capsule chips** in the header, each removable with an inline ×. New
  tags are added from a **`#` button** in the article toolbar that opens a searchable modal
  sheet — it lists the 10 most-used tags by default, toggles a tag's membership with a
  checkmark, and offers to create-and-apply a new tag as you type. (Adding lives in the sheet,
  not inline, so revealing matches never reflows the article below.)
- A divider separates the header from the scrollable body.

### Text selection

The reader supports **continuous, native selection** (drag, double/triple-click, ⌘C copy) across the
article body. SwiftUI's `Text` + `.textSelection` can only select *within* a single `Text`, so
contiguous **headings and paragraphs are coalesced into one `NSAttributedString` rendered by a
read-only `NSTextView`** (`SelectableTextView`), with the theme's spacing re-expressed as
`NSParagraphStyle` attributes. This stays within the no-WebView rule (it's AppKit/TextKit, not WebKit).

- A run breaks — forming a **selection seam** — at any block that isn't a heading or text-only
  paragraph: **images/figures, code blocks, tables, lists, and block quotes** each keep their richer
  SwiftUI renderer.
- Run height is driven by the text view's `intrinsicContentSize` (invalidated whenever the text or
  width changes), **not** `sizeThatFits` — the latter can run before the text is installed on the
  first layout pass, measuring an empty view and collapsing the run to zero height (a blank reader).
- Lists and block quotes are deliberately *not* folded into the text run for now. Their attributed
  layout (marker tab stops, hanging indents, quote bars via `ReaderLayoutManager`) is implemented in
  `MarkdownTextRun` but unvalidated on-device, so they stay on the proven SwiftUI path and form a
  seam. Re-enabling is a matter of flipping `isFoldable`.
- ⌘F find-bar isn't offered per run (a standalone `NSTextView` needs an enclosing scroll view for
  it); selection and copy are unaffected.

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
| Code blocks (with language label) | ✅ | no syntax highlighting yet |
| Tables with column alignment | ✅ | GFM |
| Thematic break / horizontal rule | ✅ | |
| Raw HTML (block & inline) | ⚠️ | text extracted, tags stripped |
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
| Tags | `tags: [..]` | labels indexed by the global search field |

The main board includes every saved item. The format-v1 `read_at`, `archived`, `favorite`, and
`rating` fields remain readable and round-trippable for compatibility with existing libraries and
older clients, but the current macOS app neither displays nor mutates them.

## Interaction model — optimistic & self-healing

Tag changes **apply to the UI instantly**; persistence happens in the background. The user clicks,
the tag flips on the next frame, and the core write + index refresh run behind it. Because the
Markdown file is the source of truth and the refresh re-reads from it, a failed write simply
reconciles back — no spinners, no manual undo.

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
