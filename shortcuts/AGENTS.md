# Shortcut workflow

Óia has two installed workflows. **Óia!** is the permanent Share Sheet entry;
**Óia Capture** is the generated helper. Capture output contract v1 is one or
more named `.cuttingscapture.zip` files followed by one confirmation text item.
The helper owns capture; Óia! owns Inbox saving and the success notification.

Preserve the existing **Óia!** identity, icon, colour, favourite placement,
accepted inputs and configured Inbox reference. Its appearance is a **red heart**.
Edit this workflow in place when migration is required. Never delete, replace,
rename or reimport it during updates. The signed Óia! artifact is a fresh-install
template, not an update package.

1. Run `make shortcut-install` for capture updates. It builds, tests, validates,
   signs and verifies **Óia Capture** only. A matching installed helper exits
   without opening Shortcuts. A stale helper blocks import alongside itself.
2. Update the helper in place or replace only **Óia Capture**, then rerun the
   installer. The permanent workflow resolves the helper through a Text action
   containing its name, so it does not depend on the helper's installed UUID.
   Keep the helper out of the Share Sheet. Leave exactly one workflow of each
   name; never choose Keep Both or create numbered copies.
3. Completion requires a verified installed helper marker and unchanged Óia!
   metadata. The installer compares the permanent workflow's installed identity,
   action count and Share Sheet support before and after helper installation.
4. For a launcher migration, export backup and result files through Apple's UI.
   Run `test-launcher.swift` against the decoded public exports to verify the
   output wiring, identical Inbox bookmark, icon and accepted inputs. Check the
   installed UUID separately. Preserve the existing Save File action's settings;
   only its input should change to the helper's archive range.
5. Close only windows opened for this task and restore the previous foreground.
   iCloud propagation, favourite placement and iPhone execution require device
   evidence; a verified Mac install does not establish them.

Apple exposes no supported silent import or replacement API. Use its native
editor/import UI; keep the private encrypted Shortcuts store untouched.
