# Save from iPhone with Shortcuts

Use **Óia!** in the iOS share sheet to save articles, images, videos, text, and
links into the `inbox` folder inside your iCloud library. You do not need an iOS
Óia app, a browser extension, an account, or a server.

## Set up

1. Open Óia on your Mac. In Settings → Library, select **Open Inbox**.
   Check that this library is inside iCloud Drive and is visible in Files on your
   iPhone.
2. For a fresh installation, add [Óia Capture.shortcut](../shortcuts/%C3%93ia%20Capture.shortcut),
   then [Óia!.shortcut](../shortcuts/%C3%93ia!.shortcut). Choose the **inbox**
   folder inside that same library when setting up Óia!. If Óia! already exists,
   preserve it and migrate its actions in place; do not import the fresh template.
3. If you install on your Mac, enable Shortcuts iCloud Sync on both devices. Both
   shortcuts can then appear on your iPhone under the same Apple Account.
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

## Updates preserve your setup

**Óia!** is a permanent Share Sheet entry with your red heart, favourite
placement and configured Inbox action. **Óia Capture** is its helper in the
Shortcuts library; it stays out of the Share Sheet and needs no favourite or
folder setup. Both sync through Shortcuts iCloud Sync.

Óia! passes the original shared items to the helper using Run Shortcut. The
helper returns one or more complete archive files followed by one contextual
confirmation text item. Óia! saves only the archives to its existing Inbox and
then displays that confirmation. The helper never saves files or shows a
success notification itself.

Future repository updates replace or edit only **Óia Capture**. The permanent
entry resolves it by name through a Text variable, rather than storing the
helper's installed identity. Its Inbox reference and favourite identity stay
untouched. A new installation may need its folder selected on iOS once; a Mac
folder selection is not proof that the reference works on an iPhone.

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

## Release or update the helper

```sh
make shortcut-install
```

This builds, tests, validates and signs **Óia Capture**, then verifies its
installed action count and side-effect-free release marker. If the installed
helper matches, it exits without importing. If a stale helper exists, it refuses
to import alongside it; update that helper in place or replace only that helper
and rerun. The installer records Óia!'s identity, action count and Share Sheet
support before and after installation and rejects unexpected changes.

`make shortcut` creates the signed helper artifact without installing it. The
permanent eight-action template is built separately with
`swift shortcuts/build-launcher.swift 'shortcuts/Óia!.unsigned.shortcut'`; sign it
with Apple's `shortcuts sign` only for a fresh-install distribution. It is never
an update package for an existing Óia!.

For an existing monolithic Óia!, use Apple's editor to migrate in place:

1. Export a backup file and record the installed Shortcut identity.
2. Retain the configured Save File action, preserving its folder reference,
   Ask Where to Save and overwrite settings.
3. Replace the capture actions with the permanent template's eight-action flow:
   Text helper name → Run Shortcut with original Shortcut Input → Count results →
   subtract one → select archive range → existing Save File → get final
   confirmation → Show Notification. Stop on absent Share Sheet input.
4. Export the result and run `test-launcher.swift result.wflow backup.wflow`
   against the decoded public export files. Verify the installed identity is
   unchanged. Never delete or reimport the permanent entry.

The helper uses output contract v1: archive files followed by one text item.
Changes to this contract require coordinated in-place launcher migration; normal
capture changes retain the contract and update only the helper. Named archives,
checksums, manifests and Rust Inbox formats are unchanged.

The Foundation generators and native action validator check action identifiers,
parameter names, enum values, variable references and control flow. Regression
checks cover capture routing and launcher output wiring, including exclusion of
the confirmation from the saved file range. `test-installed-helper.applescript`
runs an offline multi-item check without writing to the Inbox.

The migration was checked with native exports: the Inbox reference and red-heart
metadata remained identical, accepted Share Sheet input types were unchanged,
and the existing Shortcut identity remained intact. Native helper checks returned
a valid version-4 text archive with Unicode content and a separate confirmation;
a two-item run returned two archives and one plural confirmation. The permanent
launcher also ran through its helper and saved valid archives to its original
Inbox; a known existing quote retained its content identity and unchanged file.
The test archives were moved out of the Inbox to temporary storage. These Mac
checks do not establish iCloud propagation, iPhone favourites or device execution.
