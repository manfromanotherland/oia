# Shortcut workflow

When any generated Shortcut behavior changes, the work is not complete when the
`.shortcut` file is merely built or signed. The installed copy is a separate
state and must be updated as part of the same task.

Preserve the installed Shortcut's icon, colour, and Inbox destination during
updates. The current appearance is a **red heart**. Carry that appearance into
the generated release and confirm it on the installed copy; appearance changes
require a user request.

1. Run `make shortcut-install`. It builds, tests, validates, signs, and
   publishes the replacement before checking the installed Shortcut. If the
   canonical installed release already matches, it exits successfully without
   opening Shortcuts. If any stale
   **Óia!** or numbered **Óia! …** copy exists, it refuses to import, because
   importing beside an existing copy is what creates another numbered copy.
2. For a stale release, delete the canonical Shortcut and every numbered Óia
   copy once, then immediately rerun `make shortcut-install`. The agent owns
   Apple's unavoidable import UI: select **Add Shortcut** and keep or reselect
   the configured library's `inbox` response. Never use **Keep Both**, never
   import under a temporary numbered name, and never delegate editor inspection
   or version checking to the user.
3. Let the installed verifier finish. A zero exit means the installed action
   count matched and its side-effect-free probe returned the candidate's
   source-derived release marker. This proves the official generated version,
   not a byte-for-byte hash of Apple's private installed graph. Opening the
   preview, signing, or seeing one same-name tile is not completion.
4. Confirm with `shortcuts list` that exactly one canonical **Óia!** remains.
   Close only the Shortcuts windows opened for the import. If Shortcuts was not
   already open, quit it. Restore the app that was frontmost before the import;
   do not leave Shortcuts occupying the user's screen.

Apple exposes no supported silent import or replace API. Keep the UI step small
and agent-operated, but do not bypass it by editing Shortcuts' private encrypted
store. iCloud device propagation is separate from the verified Mac install;
only request an iPhone acceptance run when the task specifically requires
device-only behavior to be observed.
