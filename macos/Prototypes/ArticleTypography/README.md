# Article typography playground — throwaway prototype

Question: which body, heading, and quote fonts, sizes, leading, and spacing should Óia's reader use?

Open `typography-playground.html` directly in a browser. It is self-contained: its fonts,
illustration, licenses, styles, and controls are embedded. No server, build step, or network
connection is required. Choices stay in memory until you export them or close the page.

Or serve it from the repository root:

```sh
python3 -m http.server 8765 --bind 127.0.0.1 --directory macos/Prototypes/ArticleTypography
```

Open http://127.0.0.1:8765/typography-playground.html yourself.

- Body, headings, and quotes each have a font picker and size/leading sliders.
- H2–H6 use a visible hierarchy derived from the H1 size.
- Column width, paragraph gap, and section gap are independently adjustable.
- Light/dark appearance and hiding controls help you judge a full reading page.
- Export settings downloads JSON we can use for a later native reader implementation.
- Optional font and image uploads remain local to the page.

Fonts from UNCUT.wtf: Instrument Serif, Hedvig Letters Serif, Nyght Serif, Uncut Sans,
Instrument Sans, Geist, Inclusive Sans, and Geist Mono. The full, unmodified SIL Open Font
Licenses and creator copyright notices are inside the HTML under “Included fonts and open
licenses.” Fonts were downloaded from the creators' repositories or Google Fonts' upstream
font repository. System-font choices are also available.

This is a browser specimen for choosing typography. It does not change Óia's production reader
or claim browser and native text rendering are identical. Once Ed selects settings, transfer
and check them in the native reader. No automated browser or visual check was performed.

Captured on branch `codex/reader-typography-playground`; no design decision has been accepted yet.
