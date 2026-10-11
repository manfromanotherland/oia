-- SPDX-License-Identifier: MIT
-- Offline helper-only check. No Inbox files or notifications are produced.
on run arguments
    if (count of arguments) is not 1 then error "Expected helper name."
    using terms from application "Shortcuts"
        tell application "Shortcuts Events"
            set targetShortcut to first shortcut whose name is item 1 of arguments
            set captureResults to run targetShortcut with input {"Óia helper first fixture café 🌱", "Óia helper second fixture"}
        end tell
    end using terms from
    if (count of captureResults) is not 3 then error "Expected two archives followed by one confirmation."
    if (last item of captureResults as text) is not "2 items saved to Inbox" then error "Multi-item confirmation is incorrect."
    return "PASS: installed helper returns two archives and one plural confirmation."
end run
