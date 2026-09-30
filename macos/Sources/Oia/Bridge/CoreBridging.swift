// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// The async surface `AppState` uses to reach the Rust core. `AppState` depends
/// on this protocol rather than the concrete `CoreBridge` actor, so a test double
/// can stand in for the real core (no Rust, no filesystem) — see ADR 0001.
///
/// The methods are `async throws` here; `CoreBridge`'s actor-isolated synchronous
/// methods witness them (a call from outside an actor is implicitly async).
/// Most boundary DTOs (`Ffi*`) appear in the signatures because this is the
/// bridge. The complete board snapshot is lifted to `ReadingRow` inside the
/// bridge actor so thousands of row mappings never run on the main actor.
protocol CoreBridging: VisualSearchCore, TextTaggingCore {
    func rebuild() async throws
    @discardableResult func sync() async throws -> UInt32

    func listReadings(_ query: ReadingQuery) async throws -> [ReadingRow]
    func filterCounts(
        kind: ReadingKind?, scope: LibraryScope, tag: String?, query: String?
    ) async throws -> FfiSidebarCounts
    func getReadingRow(id: String) async throws -> FfiReadingRow?
    func getBody(id: String) async throws -> String?
    func getReadingInspector(id: String) async throws -> ReadingInspector?

    func importLink(url: String) async throws -> FfiImportResult
    func processInbox() async throws -> FfiInboxReport
    func importText(text: String, title: String?) async throws -> FfiImportResult
    func importImage(data: Data, contentType: String, title: String) async throws -> FfiImportResult
    func importVideoFile(
        filePath: String, contentType: String, title: String
    ) async throws -> FfiImportResult

    func addTag(id: String, tag: String) async throws
    func removeTag(id: String, tag: String) async throws

    func deleteReading(id: String) async throws

    func listHighlights(readingId: String) async throws -> [FfiHighlight]
    @discardableResult func addHighlight(readingId: String, text: String) async throws -> FfiHighlight
    @discardableResult func toggleHighlight(readingId: String, text: String) async throws -> Bool
    func deleteHighlight(readingId: String, highlightId: String) async throws
}

/// CoreBridge already implements every requirement; this just records the
/// conformance.
extension CoreBridge: CoreBridging {}
