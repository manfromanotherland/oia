# Chrome Web Store listing — Save to Óia

Last updated: 2026-10-15

Status: local submission draft. No store submission, dashboard update, public privacy-policy
hosting, or publisher identity has been verified by this document.

## Store listing

**Extension name:** Save to Óia

**Short description:** Save articles, links, screenshots, images, videos, and quotes to your local Óia library.

**Detailed description:**

```text
Save pages, screenshots, images, videos, and selected text to your private Óia library on your Mac.

Click the Óia toolbar button to show a compact Saving… message with a three-second countdown. Screenshot instead? appears as a link-style action to the right of the title on the same row. Nothing is saved during the countdown. When it ends, Óia saves the page as an article or link automatically. Choosing the screenshot action dismisses the message immediately and captures one long image instead. Capture stays quiet while the page scrolls. After the page returns to its original position, Saving screenshot… stays visible while Óia assembles and saves the image, then shows the result. The keyboard shortcut and a page save from the context menu use the same countdown. Right-click an image, video, or text selection and choose Save to Óia. Selected text becomes a highlight on its source article, or a quote when quotation evidence is present.

Install and open the Óia macOS app, choose your library folder, and then save from the browser. Captured content and media become ordinary files you own. The companion app and native host are required; this extension does not save to a cloud account.

Your library and recent diagnostic log stay on your device. Óia has no account system, analytics, advertising, or telemetry. Saving media can request files from the original website or its media hosts. Supported public-source saves may retrieve content from that source. Any folder syncing is your choice through a separate service.

The extension reads pages and media to capture the content you choose. It also checks page URLs against your local library to mark pages you have already saved. It does not send a browsing history to the developer.

Support and source: https://github.com/manfromanotherland/oia
Report an issue: https://github.com/manfromanotherland/oia/issues
```

**Category:** Productivity (proposed; confirm in the publisher dashboard).

**Single purpose:** Save web content and media to the user's local Óia library.

**Primary language:** English.

## Graphics and assets

| Asset | Dimensions | Status | File |
|-------|------------|--------|------|
| Store icon | 128 × 128 PNG | File and dimensions verified; manual appearance check pending | `extension/icons/icon-128.png` |
| Required screenshot | 1280 × 800 or 640 × 400 | Pending; user captures and reviews current behavior | Not supplied |
| Optional promotional tile | 440 × 280 | Not supplied | Not supplied |

Refresh the required screenshot to show the compact in-page toast with **Saving…** and the
link-style **Screenshot instead?** action to its right on the same row, above its three-second
countdown bar. The action uses white text at medium weight in dark appearance, and the countdown
is 4 px high with fully rounded ends. Include a real saved result; screenshot capture dismisses
the pending toast immediately and stays quiet while scrolling. After page restoration,
**Saving screenshot…** remains visible through image assembly and persistence, then shows the result.
Do not reuse screenshots
of the removed toolbar popup. Graphics have not been visually checked as part of this preparation.

## Permissions justification

| Permission | Type | User-facing reason |
|------------|------|--------------------|
| `scripting` | API | Capture the requested page or selected frame, display save feedback, and access the selected video's page context when the browser permits it. It also handles tabs opened before the extension was installed or reloaded. |
| `nativeMessaging` | API | Hand captures to the installed Óia companion on the same Mac so it can write the user's library files and check whether a page is already saved. |
| `contextMenus` | API | Offer Save to Óia on pages, clicked images or videos, and selected text. |
| `notifications` | API | Explain how to install the required companion when a page cannot display the in-page save message. |
| `storage` | API | Keep a local diagnostic log of recent save activity and errors so the user can inspect, copy, or clear it in the extension's options. |
| `<all_urls>` | Host | Capture user-selected web content and full-page screenshots across websites; read page URLs for the local already-saved indicator; retrieve selected media, previews, and favicons from their original hosts or CDNs. The browser's screenshot API requires this permission or activeTab, and Firefox 115 requires this permission. Pages are not sent to a developer service. |

There is no `tabs` permission. Matching host access already permits the URL, title, and favicon
properties used by the extension. No history, cookies, identity, or account permission is requested.
The broad host permission also supports the automatic local already-saved indicator; replacing it
with access only after an explicit click would change that behavior and cross-origin media saving.

## Privacy and data use

The extension **handles user data locally**. “No telemetry” is not a claim that no website content
or URLs are processed. Copy these disclosures only after checking them against the current
dashboard questions and the final release code.

| Data type | Handling | Off-device transfer | Purpose |
|-----------|----------|---------------------|---------|
| Website content | The user's chosen text, metadata, screenshots, images, and video are captured and written to the local library. | Captured content is not uploaded by Óia. Fetching source resources contacts their existing hosts. | Save content the user chose to keep. |
| Browsing activity / URLs | Current page URLs are checked against the local library; save URLs and errors can appear in the recent local diagnostic log. No separate persistent browsing-history list is created. | No developer history upload. Source retrieval requests identify the requested page or media to its host. | Show already-saved status, keep origin, and diagnose save failures. |
| Personal identifiers | Not requested as a profile or account; may appear within content the user chooses to save. | Same treatment as saved website content. | Preserve the chosen content. |
| Health information | Not separately requested; may appear within chosen content. | Same treatment as saved website content. | Preserve the chosen content. |
| Financial information | Not separately requested; may appear within chosen content, including saved product prices. | Same treatment as saved website content. | Preserve the chosen content and its metadata. |
| Authentication information | No cookie, password, or token extraction feature; visible information can still appear in a screenshot or selected content. | The browser can attach its normal session information to source requests. Óia has no account login or developer credential upload. | Retrieve the selected source with the browser's existing access. |
| Personal communications | Not separately requested; can be part of chosen page text or screenshots. | Same treatment as saved website content. | Preserve the chosen content. |
| Location | No location permission or location tracking; location can appear in chosen content. | Source hosts can observe ordinary network information such as IP address. | Retrieve the chosen resource. |
| User activity | Save actions, outcomes, and errors are kept in the last 200 local log entries; Copy and Clear are explicit user actions. | No automatic upload. A user may share copied logs separately. | Let the user diagnose saves. |

Data-use statements supported by the current code:

- Data is not sold.
- Data is used for saving, local saved-status checks, and diagnosing those features.
- Data is not used for advertising, creditworthiness, or lending.
- No analytics or telemetry endpoint is present.
- The extension does not use browser storage sync. External library-folder sync is controlled by
  the user and the chosen sync provider.

The publisher still needs to complete and certify the store's privacy-practices form. This file
does not record a completed dashboard certification.

## Privacy policy

**Source:** [docs/privacy-policy.md](docs/privacy-policy.md).

**Public policy URL:** pending hosting and live verification. A local file is not a usable
dashboard URL. Publish the reviewed policy on a public page and enter that exact URL before
submission.

## Distribution and developer information

- Visibility and regions: pending publisher selection.
- Publisher name: pending confirmation of the store account's public identity.
- Contact email: pending a verified, monitored address supplied by the publisher.
- Support: [project issues](https://github.com/manfromanotherland/oia/issues), publicly accessible
  when checked on 2026-10-11.
- Homepage: [Óia source repository](https://github.com/manfromanotherland/oia), publicly accessible
  when checked on 2026-10-11.

## Version history

| Version | Date | Changes | Status |
|---------|------|---------|--------|
| 0.1.0 | 2026-10-11 | Automatic Save, explicit screenshot capture, selected-media and text saves; separate Chromium and Firefox packages; local privacy disclosures. | Local draft; not submitted |
| 0.1.0 | 2026-10-12 | Fixed automatic product recognition and reclassification of previously saved articles. | Local draft update; not submitted |
| 0.1.0 | 2026-10-15 | Replaced the toolbar popup with a compact in-page Saving… toast and three-second countdown. The inline Screenshot instead? action uses white text at medium weight in dark appearance, above a 4 px countdown with fully rounded ends. Choosing it dismisses the pending toast immediately; capture stays quiet while scrolling, then Saving screenshot… remains visible after restoration through image assembly and persistence until the result. Otherwise the page saves when the countdown ends. Keyboard and context-menu page saves use the same countdown. | Local draft update; not submitted |

This records the current local manifest version, not a verified published version. Confirm the
existing listing's version and bump `extension/manifest.json` if required before uploading.

## Review notes and remaining release work

- Package archives exclude source, dependencies, signing keys, and this listing document.
- Chromium and Firefox receive different background manifests. The Firefox package retains
  `cuttings@edmundo.is`; the Chromium store identity must be confirmed against the companion's
  native-messaging allowlist before distribution.
- The companion currently runs on macOS. Do not advertise browser-only or Windows/Linux saving.
- A full-page screenshot requires one supported scrolling surface. Independent competing panes
  are rejected.
- A readable video is streamed into a local asset. Otherwise, recording needs a browser with
  compatible H.264 MP4 support; unsupported sources report an error. The page-world video bridge
  requires Firefox 128 or later, while the Firefox package's other paths retain the 115 baseline.
- Publish and verify the privacy-policy URL; confirm publisher identity, email, version, and
  native-host ID; supply current store screenshots; and perform real-browser acceptance before
  claiming submission readiness. These steps have not been completed by creating the ZIP.
- Store review, rejection history, and previous submissions are unknown; no dashboard state has
  been inspected or changed.

The store requires local data handling to be disclosed; see
[Chrome's user-data policy FAQ](https://developer.chrome.com/docs/webstore/program-policies/user-data-faq).
Host access already exposes matching tab metadata; see
[Chrome's Tabs permission documentation](https://developer.chrome.com/docs/extensions/reference/api/tabs#permissions).
