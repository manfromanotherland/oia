# Save from iPhone with Shortcuts

Use **Óia!** in the iOS share sheet to save articles, images, videos, text, and
links into the `inbox` folder inside your iCloud library. You do not need an iOS
Óia app, a browser extension, an account, or a server.

## Set up

1. Open Óia on your Mac. In Settings → Library, select **Open Inbox**.
   Check that this library is inside iCloud Drive and is visible in Files on your
   iPhone.
2. Open [Óia!.shortcut](../shortcuts/%C3%93ia!.shortcut) in
   Apple's Shortcuts app. Add the Shortcut and, when asked for its destination,
   choose the **inbox** folder inside that same library.
3. If you install on your Mac, enable Shortcuts iCloud Sync on both devices. The
   Shortcut can then appear on your iPhone under the same Apple Account.
   You can also send the signed `.shortcut` file to your iPhone using AirDrop.
4. On your iPhone, open the Shortcut's editor and confirm its final **Save File**
   action points to the right `inbox` folder, **Ask Where to Save** is off, and
   **Overwrite If File Exists** is off. Reselect the folder on the iPhone if
   Shortcuts asks for access.
5. Open an image in Photos, tap Share, and choose **Óia!**. Approve
   Shortcuts' first-use permission prompts. Keep the share sheet open until
   a contextual confirmation such as **Image saved to Inbox** appears.

Shortcuts supports sharing installed shortcuts across devices through
[iCloud Sync](https://support.apple.com/guide/shortcuts/apdb3a4240b0/ios),
[import questions](https://support.apple.com/guide/shortcuts/apdf330fd3a0/ios),
and [running shortcuts from another app](https://support.apple.com/guide/shortcuts/apd163eb9f95/ios).
The folder permission is yours to grant; the distributed Shortcut contains no
personal paths or folder bookmarks.

For repository updates, `make shortcut-install` builds, tests, validates, and
signs the replacement, then verifies the installed **Óia!** against its expected
official release marker. It will not open an import while an Óia Shortcut
already exists, because doing so would create a numbered copy. For a stale
release, the agent removes the old canonical Shortcut and any numbered copies
once, reruns the install, completes Apple's import UI, and leaves the final
**Save File** destination pointing to your library’s `inbox`.
Updates preserve the installed Shortcut's appearance. The generated release uses
the user's red heart icon and colour, covered by a regression check.

## What is kept

| Shared item | Saved content |
|---|---|
| Safari webpage | Page URL, title, and supplied Page Contents as a hashed HTML payload. Rust converts the body into a local Markdown article. Selected text uses the browser's highlight/quote rules and can carry the same source body. |
| Article | The sharing app's article URL, title and Body, captured as HTML for the shared Rust importer. |
| Direct image URL | URLs whose path ends in a supported image extension download into a local image attachment. The shared URL is retained as the available source. |
| Instagram post/reel | Queues the selected image or video for download on the Mac. `img_index` selects one carousel slide; no index means the first. |
| Other URL | An article request. The Mac attempts bounded public-page extraction; unavailable or unsupported pages remain links. |
| Plain or rich text | A quote, or an article request when the entire text is an HTTP(S) URL. Rich text is converted to plain text. |
| Image or video | The file representation and display name supplied by the sharing app, with a checksum. |

On the Mac, every URL-only capture passes through the Rust URL-save facade and
metadata classifier. Recognized public sources, including supported tweets,
become complete local articles with supported media saved locally. Books,
movies, music and products receive supported category metadata, covers and
prices. Ordinary version-4 article requests then use supplied page contents, or
attempt bounded public-page retrieval on the Mac when the sharing app supplies
only a URL. Missing content, blocked retrieval, and pages without usable prose
remain lightweight links. This does not change ordinary paste/drop URL saves in
the Mac app.

The Shortcut passes an explicit `capture_type` (`article`, `selection`, `text` or
`media`) in the [version-4 Inbox manifest](inbox-format.md). This records what was
shared; it does not guess a book/movie category or preassign a highlight.
Safari text with a page URL uses Rust's shared selection router: text enclosed
in matching quotation marks becomes a quote; other passages become attached
highlights. The browser can also report a quotation element, which Shortcuts
does not supply. Text with no supplied page origin remains a standalone quote.
Update the Mac app before installing this
Shortcut. Older version-1 and version-3 captures remain supported by the updated importer;
Instagram requests retain their separate version-2 format.

Every capture records its share time. Apps choose what they send to Shortcuts:
an image from Photos generally has no webpage URL, and a social app may share a
link rather than video bytes. The Shortcut does not invent a source, inspect your
clipboard, fetch a webpage, or run JavaScript. Direct HTTP(S) image URLs ending
in `.jpg`, `.jpeg`, `.png`, `.gif`, `.webp`, `.heic`, or `.heif` are downloaded
on the sharing device (including URLs with query parameters). Shortcuts may ask
for permission to contact the image host. A failed request or non-image response
stops capture instead of reporting a saved link. Extensionless image URLs remain
links. Instagram post/reel URLs use the explicit Mac download queue described
below. Safari's own **Page URL**, **Name**, **Page Contents**,
and **Page Selection** properties provide the available page context. Canonical
URLs and site names are supported by the transport but are not extracted by this
Shortcut.

The type comparisons currently use Apple's English names: **Safari Web Page**,
**Article**, **URL**, **Text**, **Rich Text**, and **Image**. They were validated against the installed
English-language Shortcuts action registry. If your iPhone uses another language,
check these If comparisons against that device's Get Type output before
relying on capture. On-device share-sheet behavior is a separate manual check.

## Instagram

The updated **Óia!** Shortcut queues an explicit media request when Instagram
shares a post/reel URL, including when supplied as plain/rich text or a Safari page
without selected text. A carousel URL with `img_index=2` saves only slide 2. If
Instagram omits the index, the first slide is selected. It never saves the whole
carousel or substitutes a link/preview when retrieval fails.

Install the downloader once on each Mac that will handle these requests:

```sh
scripts/install-instagram-downloader.sh
```

This requires Python 3 with venv/pip and installs pinned Instaloader 4.15.3 under
`~/Library/Application Support/Oia/Instagram`. It is optional for other capture
kinds and is not included in the synced library or installed at app launch.
`OIA_PYTHON` may select the Python executable during setup. Re-run setup if a
Python upgrade invalidates the environment. The app bundles the small transport
adapter, not Python itself.

Keep Óia open on the Mac with internet access. After iCloud delivers the request,
Óia downloads the selected media and imports its local asset. **Instagram media
queued in Inbox** on iPhone confirms the request was queued, not that the download
finished.
Failures remain visible in Settings → Library; **Check Inbox** retries them.
There is no third-party download service and no automatic account login. Some
posts may require authentication or be unavailable; those remain queued with an
error rather than silently becoming links. Live photo capture has been verified;
video selection/import is covered by fixtures and still needs an iPhone reel test.

## When Óia imports

Each shared item is saved as one `<capture-id>.cuttingscapture.zip` archive.
Keeping the manifest and media together prevents separate files arriving through
iCloud in the wrong order. The Shortcut saves the archive; it does not write
Óia' permanent library files.

Óia imports when the Mac app is open, or after you next open it. It requests
missing iCloud downloads, waits for settled files, verifies the archive and media
checksum, and uses the Rust importer to create the normal Markdown and asset
files. The Inbox copy is removed only after the saved item and local assets have
been verified. An interrupted import can be retried without duplicating cards.

A confirmation such as **Image saved to Inbox**, **Link saved to Inbox**,
**Selection saved to Inbox**, or
**3 items saved to Inbox** means the capture was saved on your iPhone, not that
iCloud has finished syncing or that Óia has imported it. There is no background
Mac service when Óia is closed.

Use Settings → Library → **Check Inbox** to retry. Files that cannot be imported
stay in the Inbox, with an explanation in Settings; they are not deleted.
Unsupported files and folders are not silently converted. Ordinary supported
image, video, text, `.url`, and `.webloc` files can also be saved directly into
`inbox` with Files or Finder, without the Shortcut's extra source metadata.

## Manual check on iPhone

Before relying on this workflow, share one of each:

- A Safari article with no selected text: check that the saved Markdown contains
  its body and retains the source URL and title. Repeat while offline when the
  sharing app supplies Page Contents.
- An ordinary article URL from another app: check the Mac's extracted body; an
  unavailable page or navigation-only site should retain a link.
- A book, movie, music or product URL: check the category and supported metadata
  after Mac import, including a cover or price when the source provides one.
- A supported tweet URL: check the full text and available local photos.
- Selected text in a Safari article: check the attached highlight and its source
  URL. Repeat the share to check highlight deduplication. Some share paths
  provide only plain text, in which case it remains a standalone quote.
- Long-press a website image and share its direct `.jpg` URL: confirm the card
  is an image and its local asset opens.
- A photo, then a short video: open the imported local assets on the Mac.
- Plain text containing quotes, line breaks, and emoji: check the saved text.
- Several photos at once: confirm one archive and one card per item.

Close Óia before one test, then reopen it. The archives should remain in the
Inbox until the import succeeds. Share an identical item again to check that it
deduplicates and leaves no stale Inbox archive.

## Release or update the Shortcut

The signed file is the installable artifact. The unsigned XML and generator are
kept alongside it so the workflow can be reviewed without importing it.

```sh
make shortcut-install
```

This builds the reviewable XML in a temporary directory, runs the regression and
native-action validators, signs it, and publishes both artifacts only after every
check passes. If the installed canonical release already matches, it exits
without opening Shortcuts. If any stale **Óia!** or numbered **Óia! …** copy is
installed, it refuses to import and prints the conflicts. The agent removes those
copies once and reruns the command; only then does it open the signed file and
wait for the **Add Shortcut** import. This prevents **Keep Both** and numbered
copy churn rather than trying to clean it up after every import. The agent does
not ask the user to inspect the editor or attest to the result.

Signing is not installation. The generated workflow contains an early,
side-effect-free version probe. Before running it, the verifier rejects every
known unsafe release and checks the installed action count, so an older
official workflow is never run as a probe. The current probe runs only when the
Shortcut has no input; ordinary Share Sheet saves always have input. The
verifier runs the installed Shortcut through Apple's background runner and
compares its source-derived release marker with the signed candidate.
A zero exit therefore means:

1. Exactly one installed **Óia!** exists and accepts Share Sheet input.
2. Its action count matches the candidate, so the version probe is safe to run.
3. Its probe returns the candidate's SHA-256 release marker before any save or
   notification action.

The marker proves which official generated release is running; Apple does not
expose the installed action graph or the configured folder bookmark for hashing.
The agent must therefore keep or select the known library `inbox` in the import
UI. That setup responsibility is not delegated to the user.

Use `make shortcut` when only a signed distributable is needed. It deliberately
prints that the installed behavior remains unverified. Apple exposes no supported
silent import API, so the small import UI step remains agent-operated. The preview
opens in the background; after verification the agent closes Shortcuts and
restores the app that was previously in use. iCloud propagation and device-only
rendering are separate from this Mac-side installation proof and are checked only
when a change specifically needs on-device acceptance.

The generator uses only Foundation. The developer validator uses Apple's local
Shortcuts action registry to check action identifiers, parameter names, enum
values, retained variable references, If subjects, balanced control flow, and the
destination import question. It also guards two native execution pitfalls:
**Make Archive** can ignore its name field, so the archive is explicitly renamed;
**Set Dictionary Value** unwraps a one-item List, so the attachments array is
created by parsing JSON before setting the other manifest fields.

The validator does not install or run the Shortcut. Signing uses Apple's
`shortcuts sign` helper and may require access outside a restricted terminal
sandbox. A successful signature does not update an already installed Shortcut;
the installed release-marker handshake above is the release gate.

Before the direct-image-URL change, the generated workflow was also run in Mac Shortcuts with typed text, a URL,
and a PNG. Two runs produced six archives with correct names and JSON types.
The current Rust importer saved three items, recognized the other three as
duplicates, and cleared only the successfully imported temporary Inbox copies.
Replaying all six produced six duplicates and no extra items. The test verified
the source URL, Unicode and line breaks, image display name, and identical source,
archive, and imported-image checksums. Safari, Photos, video sharing, and iCloud
delivery on iPhone still need the device checks above.

The running Mac app's iCloud Inbox watcher also completed a duplicate handoff
automatically after the file settled, without altering the existing reading.
