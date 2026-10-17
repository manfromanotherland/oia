# Product metadata

Products are a presentation subtype of article readings, like social posts. They stay in Links;
their URL-derived IDs, local previews, bodies, and Tags are unchanged. Product recognition does
not create a Tag or a fifth reading kind.

## Detection and price

1. Prefer the page's Schema.org `Product` or `ProductGroup` JSON-LD and its associated `Offer`.
   Resolve graph references, select the product belonging to the current page, and reject ambiguous
   collections or incidental products inside an article/review.
   When a Product has no page identity, a uniquely matching URL in its associated Offer can identify
   it. An Offer does not override a conflicting identity declared by the Product.
2. Fall back to scoped Schema.org microdata/RDFa, then explicit product Open Graph metadata.
   A currency symbol, a number in visible text, or `/products/` in the URL is insufficient evidence.
3. Record the offered price as a decimal string with its currency code. A minimum price from an
   `AggregateOffer` is labelled **From**. Product evidence can classify a card without a usable
   price; the card then says **Product**.
4. Persist a versioned `product` record in the Markdown frontmatter, including the capture time.
   SQLite and the Swift row projection remain rebuildable caches. The price is a saved snapshot;
   browsing the library does not poll stores for price changes.

The browser extension's **Save** action collects bounded raw metadata from the live DOM.
Rust owns recognition and validation. This also supports metadata inserted by client-side scripts,
without making the native host fetch the page again. Ordinary URL-only saves, including the
Shortcut, can attempt a bounded public HTML fetch and remain normal lightweight links if offline,
blocked, or missing reliable metadata.

Recognized products also supply their main product title and image from the same page snapshot.
Structured product images take precedence over the page's Open Graph image. The extension captures
the selected image in the browser; URL-only saves retrieve it through the bounded public artwork
fetcher. Both paths store image bytes in the reading's local assets and persist only a relative
`preview_asset`. An unavailable image does not prevent saving the product or its price.

The card fits its local preview inside a square or gently portrait canvas. The saved price appears
in a theme-aware top-right badge, with a larger title over a dark gradient at the bottom of the
image and no source footer. Products without previews use a neutral surface and semantic title
color. Currency formatting belongs to the native presentation layer.

## Existing library

Re-saving a recognized product can enrich an existing full article as well as a lightweight link.
The update preserves the original body, assets, title, save date, Tags, and user state; a fingerprint
check under the reading lock prevents stale captures from overwriting an external edit. The product
metadata moves the card into Links without discarding the earlier capture.
When a recognized product has no usable local preview, re-saving it can fill that preview while
preserving the existing article file's other frontmatter and body. A usable saved preview is kept.

The importer has a separate product enrichment mode that scans existing lightweight article links,
including those already enriched with previews. It leaves unrecognised/unreachable links alone and
does not delete anything. Preview is the default; writing uses a separate backup directory and
revalidates the original file before attaching product metadata. The report records coverage and
skips, so blocked sites are not presented as successfully inspected.

## Sources and limits

[Schema.org Product](https://schema.org/Product) and [Offer](https://schema.org/Offer) describe
product identity and commercial offers. [Google's product structured data documentation](https://developers.google.com/search/docs/appearance/structured-data/product-snippet)
documents product/offer prices, currencies, and aggregate offers in JSON-LD, microdata, and RDFa.

[mymind's own help](https://mymind.helpscoutdocs.com/article/61-what-type-of-things-can-i-save)
confirms automatic product cards with visible prices. Its
[description of automatic categorisation](https://mymind.com/so-magical-you-didnt-see-it)
does not disclose the parser or selection algorithm. Structured metadata is our implementation
choice; it is not a claim about mymind's internals.

Prices can vary by variant, destination, currency, tax, or store session. Live browser metadata
usually provides better context than a later HTTP migration. This first version deliberately
leaves uncertain prices absent instead of inferring them from arbitrary text.
