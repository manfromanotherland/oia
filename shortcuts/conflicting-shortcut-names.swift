// SPDX-License-Identifier: MIT
// Apple can return decomposed names while generated workflows use composed names.
import Foundation

let target = CommandLine.arguments.dropFirst().first.flatMap { $0 == "--test" ? nil : $0 } ?? "Óia Capture"
func matchesShortcut(_ name: String) -> Bool {
    let normalized = name.precomposedStringWithCanonicalMapping
    return normalized == target || normalized.hasPrefix(target + " ")
}

if CommandLine.arguments.dropFirst().first == "--test" {
    let matches = ["Óia Capture", "O\u{0301}ia Capture", "Óia Capture 2", "O\u{0301}ia Capture 3"]
    let others = ["New Shortcut", "Óia!", "Óia", "Óia CaptureElsewhere"]
    precondition(matches.allSatisfy(matchesShortcut) && !others.contains(where: matchesShortcut))
    print("PASS: composed and decomposed canonical/numbered helper names, separate from the permanent Óia! are detected")
} else {
    while let name = readLine() {
        if matchesShortcut(name) { print(name) }
    }
}
