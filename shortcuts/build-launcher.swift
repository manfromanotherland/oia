// SPDX-License-Identifier: MIT
// Fresh-install template only. Existing Óia! workflows are edited in place.
import Foundation

guard CommandLine.arguments.count == 2 else {
    fatalError("Usage: swift shortcuts/build-launcher.swift <destination>")
}
let source = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("launcher.json")
let workflow = try JSONSerialization.jsonObject(with: Data(contentsOf: source))
let bytes = try PropertyListSerialization.data(fromPropertyList: workflow, format: .xml, options: 0)
try bytes.write(to: URL(fileURLWithPath: CommandLine.arguments[1]), options: .atomic)
print("Built permanent Óia! template (fresh installs only).")
