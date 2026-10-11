// SPDX-License-Identifier: MIT
// Apple can return decomposed names while generated workflows use composed names.
import Foundation

func isOiaShortcut(_ name: String) -> Bool {
    let normalized = name.precomposedStringWithCanonicalMapping
    return normalized == "Óia!" || normalized.hasPrefix("Óia! ")
}

if CommandLine.arguments.dropFirst().first == "--test" {
    let matches = ["Óia!", "O\u{0301}ia!", "Óia! 2", "O\u{0301}ia! 3"]
    let others = ["New Shortcut", "Óia", "Óia!Elsewhere"]
    precondition(matches.allSatisfy(isOiaShortcut) && !others.contains(where: isOiaShortcut))
    print("PASS: composed and decomposed canonical/numbered Óia names are detected")
} else {
    while let name = readLine() {
        if isOiaShortcut(name) { print(name) }
    }
}
