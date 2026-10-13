# Article typography: Biblioteca

Native adaptation of the Biblioteca theme installed with NetNewsWire, by Martín González Gómez.
Reference source: `/Applications/NetNewsWire.app/Contents/Resources/Biblioteca.nnwtheme/stylesheet.css`.

- Platform serif body and article title (`ui-serif`); sans-serif H2–H6 (`ui-sans-serif`).
- Body sizes: 16, 18, 20, 22, 24 pt; Óia also retains a 26 pt Giant option.
- Normal leading: 1.6; Snug: 1.475, corresponding to Biblioteca's compact layout.
- Bold title/H1: 2em. H2: 1.5em, H3: 1.17em, H4: 1em, H5: 0.83em, H6: 0.67em.
- Headings: 1.2 leading, 1.5em above, 0.25em below (relative to heading size).
- Paragraphs: collapsed 1em margins. A paragraph immediately following a heading
  uses the heading's smaller bottom margin.
- Quotes: ordinary upright body text, 15 pt left padding plus a subtle 3 pt rule.
- List items: 0.5em separation.
- Article title is left aligned. Tags are omitted from the article header.

Óia keeps its native Markdown renderer, semantic light/dark colors, local assets,
continuous text selection, and adjustable font/size/width/leading preferences.
Existing stored preference raw values are preserved. No webfonts or WebView are needed.
The reference's feed header and dateline are not added to Óia's article header.
