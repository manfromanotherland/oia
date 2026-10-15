<p align="center">
  <img src="icons/icon-128.png" alt="Óia" width="128">
</p>
<h1 align="center">Óia</h1>
<p align="center">
  For your eyes only
</p>

---

# Browser extension

The Manifest V3 browser extension for **Óia**, the local-first native macOS library. Clicking its
toolbar button shows a compact in-page toast with **Saving…** and a link-style **Screenshot instead?**
action to its right on the same row, above a three-second countdown bar. Nothing is saved until
the countdown ends; then the engine uses the page's content and source to keep it as a cleaned article or a lightweight
link. Choosing the screenshot action dismisses the pending toast immediately and saves one long
screenshot of the full scrollable page instead, with only a result notification when finished.
The context menu also saves right-clicked
images/videos and selected-text quotes. Article and link saves
retain live page metadata, social previews, and favicons as local assets. Captures travel through the
`is.edmundo.cuttings.host` native messaging host and become Markdown plus local assets in your
library folder.

Every right-clicked video uses protocol v4's streaming import. The extension reads HTTP(S), `data:`, and
document-scoped `blob:` sources from the live page; if a source cannot be fetched, it captures one
loop of the exact rendered video using an explicitly supported H.264 MP4 recorder. It never falls
back to a WebM that the native app cannot play. Raw bytes travel in chunks of at most 256 KiB over
one long-lived tab port. The worker relays each chunk over one native connection and waits for its
acknowledgement before allowing the next one through. It never builds a whole-video `ArrayBuffer`
or persists a temporary source URL. The native host writes the finished movie as the card's local
playable asset; failed reads, recordings, stream operations, or native writes abort the upload and
surface a save error. A poster-only capture is never reported as a successful video save.

## Build

```bash
npm install
npm run build
```

## Package for each browser

```bash
npm run package
```

Builds a fresh bundle, then copies only runtime files
(`manifest.json`, the four runtime bundles in `dist/`, `icons/`, `options.html`, `install.html`)
into these outputs:

- `unpacked/` — a stable clean directory for **Load unpacked** in Dia/Chrome during development.
- `unpacked-firefox/` — the Firefox build for **Load Temporary Add-on**.
- `artifacts/oia-extension-chrome-<version>.zip` — the Chromium archive for the Chrome Web Store
  or Edge Add-ons.
- `artifacts/oia-extension-firefox-<version>.zip` — the Firefox archive for
  [AMO](https://addons.mozilla.org).
- `artifacts/oia-extension-<version>.zip` — an identical Chromium archive retained for existing
  release scripts.

Chromium uses a Manifest V3 service worker; Firefox uses a Manifest V3 event page
(`background.scripts`, `persistent: false`). The Firefox manifest retains the add-on ID
`cuttings@edmundo.is` so the installed native host recognizes it. The Chromium archive omits
Firefox-specific manifest fields. Both archives contain bundled code, not source, dependencies,
or store preparation documents.

Bump the `version` in `manifest.json` before store packaging; stores reject a re-upload of an
existing version. Requires `zip` (preinstalled on macOS).

The store archives drop the `key` field. `unpacked/` keeps it to pin the development extension ID;
Firefox uses its explicit add-on ID. Store signing determines the published Chromium identity.
Confirm that identity in the native host's allowed origins before distribution.

Packaging does not submit or publish a listing. [CHROMEWEBSTORE.md](../CHROMEWEBSTORE.md) records
the listing copy, permission explanations, privacy disclosures, and remaining submission work.
The [privacy policy source](../docs/privacy-policy.md) must be hosted at a public URL before it
can be entered into the store dashboard. Store screenshots and real-browser acceptance remain
manual checks.

## Load in the browser

**Chrome / Edge**

1. Open `chrome://extensions` and enable **Developer mode**.
2. Run `npm run package`, then **Load unpacked** → select `extension/unpacked/`.

After editing source, re-run `npm run package` and click the **reload ↻** icon on the extension
card. Dia/Chrome keeps the same unpacked path and stable development extension ID.

Click the Óia toolbar button to show **Saving…** in the compact in-page toast. The link-style
**Screenshot instead?** action sits to its right on the same row, in white text at medium weight
in dark appearance. The countdown bar is 4 px high with fully rounded ends and gives you three
seconds to choose it. When it ends, Óia saves the page. Choosing the screenshot action dismisses
the pending toast immediately and saves the full scrollable page as one long image instead;
only the result notification appears when finished. Nothing is saved during the countdown. The keyboard shortcut and a page save
from the context menu use the same countdown.

On app-style pages, Óia can follow one scrolling surface that fills the browser viewport. If a
page has multiple large, independently scrolling panes, capture stops with an error rather than
saving a misleading one-viewport image.

**Firefox**

1. Run `npm run package`.
2. Open `about:debugging#/runtime/this-firefox`.
3. **Load Temporary Add-on…** → select `extension/unpacked-firefox/manifest.json`.

Use the generated Firefox build, rather than the source Chromium manifest. Reload the temporary
add-on after repackaging. Firefox 115 or later is the baseline; permit site access when Firefox
requests it. Full-page screenshot capture retains `<all_urls>` because Firefox 125 and earlier
require that permission for `tabs.captureVisibleTab`.

The page-world bridge for otherwise unreadable temporary videos requires Firefox 128 or later.
Readable video sources still use the ordinary streaming path on the baseline. Recording an
unreadable video also requires the browser to advertise compatible H.264 MP4 recording; when that
capability is absent, the save reports an error rather than writing an unplayable movie.

Temporary add-ons are cleared on restart; a persistent install needs a signed build from
[AMO](https://addons.mozilla.org).

Browser manifest and API compatibility references:
[Firefox background scripts](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/background#browser_support),
[screenshot permissions](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/tabs/captureVisibleTab),
[Firefox MV3 Promise support](https://bugzilla.mozilla.org/show_bug.cgi?id=1711570), and
[page-world scripting support](https://github.com/mdn/browser-compat-data/blob/main/webextensions/api/scripting.json).

## Wire the extension ID to the native host

Saves travel over **native messaging**, which only connects extensions whose ID is listed in the
native host's manifest. The app registers the stable Chromium development ID and Firefox add-on
ID when it launches. An unregistered Chromium ID cannot save; the extension reports that the host
is unavailable. To register a different Chromium or store ID:

1. Load the unpacked extension (above).
2. On `chrome://extensions`, copy the extension **ID** (32 lowercase letters).
3. Register it with the host (see the [core README](../core/README.md)):

   ```bash
   ../core/target/release/oia-native-host --install-manifest --extension-id <your-32-char-id>
   ```

Firefox needs no Chromium ID step. Open the Óia app once so it registers the native host under
Firefox's native-messaging directory, using `allowed_extensions: ["cuttings@edmundo.is"]`.
Install the Firefox browser before opening the app, or reopen the app afterward so it discovers
the browser. The companion host runs on macOS; the browser add-on alone cannot write a library
on another operating system.

> The Óia app must have installed its native messaging host. The browser starts the host as needed.

## Extraction

Saving a page runs three stages in `src/extraction.ts`: DOM pre-processing (including optional
per-host **site adapters** in `src/site-adapters/`), Mozilla Readability to select the main
content, and Turndown to convert it to Markdown.

## Test & lint

```bash
npm test
npm run lint
npm run lint:fix
```

### Payment-required pages

Article and link saves stop with a “Payment required” error toast when the loaded
page’s Navigation Timing entry reports HTTP 402. No save request reaches the
native host, so no card is created or overwritten. This checks the browser’s
existing response without fetching the page again. Browsers that do not expose
`responseStatus` cannot detect this condition; paywalls served with HTTP 200 are
not detected by this status check.
