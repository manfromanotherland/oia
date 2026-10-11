# Privacy policy for Save to Óia

Last updated: 2026-10-11.

Publication status: policy source prepared for review. A public hosted URL and publisher contact
email still need to be confirmed before store submission. The current support contact is the
[Óia project issue tracker](https://github.com/manfromanotherland/oia/issues).

## What Óia handles

Save to Óia saves web content to a library folder on your Mac through the installed Óia companion.
When you choose to save, it handles the source URL, page title and metadata, selected or extracted
text, screenshots, images, and video needed for that saved item. Saved content can contain personal
or sensitive information if you choose to save a page that contains it.

Óia also checks visited page URLs against your local library to show whether a page has already
been saved. Those checks do not send a browsing history to the developer. A recent diagnostic log
records save actions, URLs, outcomes, and error details locally so you can investigate a failed save.

The extension does not create a user account or request your location, browser history, cookies,
passwords, or authentication tokens as separate data sources. Information visible in a screenshot
or included in selected page content can still become part of the item you save.

## Where it is stored

Your library is made of Markdown files and local media in the folder you choose. The companion's
search index is a local, rebuildable cache outside that folder. The extension stores its last 200
diagnostic log entries in the browser's local extension storage; it does not use browser storage
sync.

Óia does not provide its own sync service. If you place your library in a folder synced by iCloud,
Dropbox, Google Drive, git, or another service, that service handles the files under its own terms
and privacy policy. You control that choice.

## Network requests and sharing

Óia has no developer-operated library server, advertising service, analytics, or telemetry. It does
not sell your data or use it for advertising, creditworthiness, or lending. User data is used only
to save your chosen content, show local saved status, and diagnose those features, consistent with
the Chrome Web Store User Data Policy's limited-use requirements.

Saving media can request its bytes from the original website, a media host, or a content-delivery
network. A supported public-source save may request page data and media from that source. Those
hosts receive normal network information, such as the requested resource and your IP address, and
the browser may use its existing session access where appropriate. Their responses are saved
locally; Óia does not upload your library to those hosts.

Captures are passed from the extension to the Óia companion on the same Mac. That local handoff is
not an upload to the developer. Diagnostic logs are not automatically sent anywhere. If you choose
Copy in the extension's options and share the result, the destination you choose receives it.

## Retention and your controls

Saved items remain in your library until you delete them in Óia or remove their files. You can read,
copy, export, or back up those ordinary files independently of the app. Any external sync provider
may retain copies according to its own deletion and retention rules.

The diagnostic log is limited to the latest 200 entries. Open the extension's options and choose
Clear to remove it; Copy places it on your clipboard only when you choose that action. Removing
the extension removes its browser-managed local storage, while your library folder remains yours.

You can restrict site access or remove the extension through your browser. Captures need access
to the page and media you ask to save, so restricting that access can prevent a save.

## Policy changes and contact

Changes to these practices will be reflected in this policy and its update date. Before publishing
a store release, the publisher must ensure the hosted policy and store disclosures match the
released behavior.

For questions, contact the maintainer through the
[project issue tracker](https://github.com/manfromanotherland/oia/issues). It is public: avoid
posting private saved content, credentials, or diagnostic logs there. A verified contact email
will be added when the publisher supplies one.
