<p align="center">
  <img src="icons/icon-128.png" alt="Óia" width="128">
</p>
<h1 align="center">Óia</h1>
<p align="center">
  For your eyes only
</p>

---

# Browser extension

The Manifest V3 browser extension for **Óia**, the local-first native macOS library. Its toolbar
saves a cleaned article, a lightweight link, or one long screenshot of the full scrollable page. The
context menu also saves right-clicked images/videos and selected-text quotes. Article and link saves
retain live page metadata, social previews, and favicons as local assets. Captures travel through the
`is.edmundo.cuttings.host` native messaging host and become Markdown plus local assets in your
library folder.

Every video uses protocol v4's streaming import. The extension reads HTTP(S), `data:`, and
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

## Package for the stores

```bash
npm run package
```

Builds a fresh bundle, then copies only the files the manifest ships
(`manifest.json`, `dist/`, `icons/`, `popup.html`, `options.html`, `install.html`) into two outputs:

- `unpacked/` — a stable clean directory for **Load unpacked** in Dia/Chrome during development.
- `artifacts/oia-extension-<version>.zip` — ready for the Chrome Web Store, Edge Add-ons, or
  [AMO](https://addons.mozilla.org).

Bump the `version` in `manifest.json` before store packaging; stores reject a re-upload of an
existing version. Requires `zip` (preinstalled on macOS).

The packaged manifest drops the `key` field — it pins a stable extension ID for
local unpacked development, but the Chrome Web Store manages signing itself and
rejects any upload that carries `key`. `manifest.json` on disk keeps it.

## Load in the browser

**Chrome / Edge**

1. Open `chrome://extensions` and enable **Developer mode**.
2. Run `npm run package`, then **Load unpacked** → select `extension/unpacked/`.

After editing source, re-run `npm run package` and click the **reload ↻** icon on the extension
card. Dia/Chrome keeps the same unpacked path and stable development extension ID.

Click the Óia toolbar button and choose **Save article**, **Save link**, or **Save screenshot**.
The screenshot action captures the full scrollable page as one long screenshot. The existing
keyboard shortcut saves the full article directly.

On app-style pages, Óia can follow one scrolling surface that fills the browser viewport. If a
page has multiple large, independently scrolling panes, capture stops with an error rather than
saving a misleading one-viewport image.

**Firefox**

1. Open `about:debugging#/runtime/this-firefox`.
2. **Load Temporary Add-on…** → select this folder's `manifest.json`.

Temporary add-ons are cleared on restart; a persistent install needs a signed build from
[AMO](https://addons.mozilla.org).

## Wire the extension ID to the native host

Saves travel over Chrome **native messaging**, which only connects extensions whose ID is listed
in the native host's manifest. Until you register your ID, saves silently fail (extraction still
works). To wire it up:

1. Load the unpacked extension (above).
2. On `chrome://extensions`, copy the extension **ID** (32 lowercase letters).
3. Register it with the host (see the [core README](../core/README.md)):

   ```bash
   ../core/target/release/oia-native-host --install-manifest --extension-id <your-32-char-id>
   ```

Firefox needs no ID step — it matches the host by the fixed add-on ID baked into the manifest.

> The native messaging host must be installed and running for saves to work.

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
