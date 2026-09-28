# Changelog

All notable changes to Óia (the macOS app users download) are recorded
here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions track the app's `CFBundleShortVersionString`. See
[RELEASE.md](./RELEASE.md) for how a release is cut.

## Unreleased

### Added

- Instagram post and reel shares through **Óia!** can now queue a local Mac
  download of the selected photo/video. Carousel shares preserve `img_index`;
  failures remain in Inbox for retry instead of becoming link cards. Requires
  the per-device Instaloader setup described in the Shortcut guide.

- Inbox imports images, videos, text, and links from the library's `inbox` folder
  while Óia is open. The iOS **Save to Óia** Shortcut keeps available
  source details with each capture in iCloud Drive. Successful inputs are removed
  only after the saved content is verified; failed inputs remain for retry.
- Links, text, images, videos, and local `.txt`/`.md` files can now be saved by dropping
  them on the card board or pasting with ⌘V. URL-only saves remain explicitly
  lightweight until a later browser capture upgrades them in place. Local videos are copied into
  the library and play directly in Óia without depending on the original file.
- Settings › Typography can now set the reader's **Width** — the measure the
  article is laid out to — from Extra Small (520 pt) through Extra Large
  (960 pt), with Medium (680 pt) the previous fixed value and still the default.
- Settings › Typography can now set the reader's **Line Height** — Tight, Snug,
  Normal, Relaxed, or Loose (1.25 to 2.25 in even quarter-steps), with Normal
  (1.75) the previous fixed value and still the default.
- The Typography settings show a live sample that reflects the chosen font, size,
  width, and line height together.
- Both are also adjustable from the appearance popover at the bottom of the
  sidebar, without opening Settings: two icon-capped sliders matching the
  font-size slider already there.

### Changed

- Full-article board cards now show a description and estimated reading time in the reader's
  Palatino hierarchy. Missing descriptions use the start of the saved article text, and text-only
  cards fit their content without large empty areas. Article cards use 24 pt padding; quote cards
  use italic Palatino with 24 pt side and 16 pt vertical padding.

- Card details now have a floating Liquid Glass inspector with Discover and Details tabs,
  compact tag pills, clickable image attributes and closely spaced colour swatches. Swatches
  search for similar shades; file details show readable metadata instead of internal asset URLs.

- **Óia!** confirmations now name the saved item—such as an image, link, or
  quote—without repeating the product name in the notification.
- The iOS Shortcut is now named **Óia!**. Direct image URLs shared from Safari
  download as local image captures instead of becoming lightweight links.

- Renamed the app to **Óia** and the browser extension and iOS Shortcut to
  **Save to Óia**. Existing libraries, preferences, and browser identities are
  preserved.
- The sidebar filters now narrow in order — smart view, then rating, then tag,
  as the sidebar reads top to bottom. Changing one clears the narrower ones
  below it, so switching from ★5 to ★4 drops the tag you had applied, and
  going from Read back to All drops both. Previously they changed
  independently, which left the list scoped by a combination you hadn't asked
  for — often empty, for a reason hidden in a collapsed section.

### Removed

- Favorite and personal-note controls have been removed from the macOS app. Existing `favorite`
  metadata and `note.md` sidecars remain untouched for compatibility with older libraries.

### Fixed

- The article header now shares the reader's measure instead of a fixed 680 pt,
  so the title stays flush with the body copy at every width.

## 0.2.0 - 2026-08-10

### Added

- Onboarding and Settings › Extensions link to the browser extension on the
  Chrome Web Store and Firefox Add-ons, now that the listings are public.

### Changed

- Onboarding is now a compact two-step sheet (choose a library, then add the
  extension); the main window opens larger on first run and keeps the size you
  leave it at afterward.
- Refreshed the welcome article copy.

### Removed

- The bundled extension download and its load-unpacked (sideload) instructions,
  replaced by the store links above.

## 0.1.1 - 2026-08-07

### Added

- Onboarding step after choosing a library: download the browser extension and
  load it unpacked, with a link to the source at
  [`extension/`](./extension).
- Settings › Extensions offers the same extension download and load-unpacked
  instructions for Chrome/Edge/Brave and Firefox.

### Changed

- Settings › Extensions no longer links to the Chrome Web Store / Firefox Add-ons
  listings while the extension is still under review; it hands out the packaged
  download instead.

## 0.1.0

- Initial release: browse, read, search, tag, and rate your library; native
  SwiftUI reader; in-app updates via Sparkle.
