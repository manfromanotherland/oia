// SPDX-License-Identifier: GPL-3.0-or-later

import CryptoKit
import Foundation

/// A single fake reading, rendered to the exact on-disk format the Rust core
/// parses (`core/src/frontmatter.rs` + `types.rs`): YAML frontmatter followed by
/// a Markdown body. Required fields are always emitted; optional fields only
/// when set, matching the core's `#[serde(skip_serializing_if = "Option::is_none")]`
/// and `#[serde(default)]` rules.
struct ArticleFixture {
    // Required frontmatter.
    var id: String
    /// Omitted for a legacy article; the core defaults a missing kind to article.
    var kind: String?
    /// Only emitted for a URL-only link placeholder.
    var lightweight: Bool
    var url: String
    var canonicalURL: String
    var title: String
    var savedAt: Date
    var archived: Bool
    var favorite: Bool
    var tags: [String]

    /// Optional frontmatter — emitted only when non-nil / non-zero.
    /// Presence marks the reading read (there is no separate `read` boolean).
    var readAt: Date?
    /// 0 means unrated; only emitted when > 0 (the core defaults a missing key to 0).
    var rating: UInt8
    var author: String?
    var site: String?
    var excerpt: String?
    var wordCount: Int?
    var lang: String?
    /// Indented YAML fields below `source_profile:` for an existing captured source.
    var sourceProfileYAML: String?

    /// The Markdown body (the `# Title` heading + content).
    var body: String

    init(
        id: String,
        url: String,
        title: String,
        savedAt: Date,
        kind: String? = nil,
        lightweight: Bool = false,
        canonicalURL: String? = nil,
        archived: Bool = false,
        favorite: Bool = false,
        tags: [String] = [],
        readAt: Date? = nil,
        rating: UInt8 = 0,
        author: String? = nil,
        site: String? = nil,
        excerpt: String? = nil,
        wordCount: Int? = nil,
        lang: String? = nil,
        sourceProfileYAML: String? = nil,
        body: String? = nil
    ) {
        self.id = id
        self.kind = kind
        self.lightweight = lightweight
        self.url = url
        self.canonicalURL = canonicalURL ?? url
        self.title = title
        self.savedAt = savedAt
        self.archived = archived
        self.favorite = favorite
        self.tags = tags
        self.readAt = readAt
        self.rating = rating
        self.author = author
        self.site = site
        self.excerpt = excerpt
        self.wordCount = wordCount
        self.lang = lang
        self.sourceProfileYAML = sourceProfileYAML
        self.body = body ?? "# \(title)\n\nBody of \(title).\n"
    }

    /// `sha256:<hex>` over the body bytes. The scanner also keys on file mtime,
    /// so an exact match with the core isn't required for indexing — but this
    /// produces a spec-valid `source_hash`.
    var sourceHash: String {
        "sha256:" + Self.sha256Hex(body)
    }

    /// The full file contents: frontmatter fence, fields, fence, blank line, body.
    func rendered() -> String {
        // The core's documented field order; an optional field whose value is nil
        // drops out (matching the core's `skip_serializing_if`).
        let fields: [(String, String?)] = [
            ("format_version", "1"),
            ("id", quoted(id)),
            ("kind", kind.map(quoted)),
            ("lightweight", lightweight ? "true" : nil),
            ("url", quoted(url)),
            ("canonical_url", quoted(canonicalURL)),
            ("title", quoted(title)),
            ("author", author.map(quoted)),
            ("site", site.map(quoted)),
            ("saved_at", quoted(Self.timestamp(savedAt))),
            ("read_at", readAt.map { quoted(Self.timestamp($0)) }),
            ("archived", "\(archived)"),
            ("favorite", "\(favorite)"),
            ("rating", rating > 0 ? "\(rating)" : nil),
            ("tags", "[\(tags.map(quoted).joined(separator: ", "))]"),
            ("excerpt", excerpt.map(quoted)),
            ("word_count", wordCount.map { "\($0)" }),
            ("lang", lang.map(quoted)),
            ("source_profile", sourceProfileYAML.map { "\n\($0)" }),
            ("source_hash", quoted(sourceHash))
        ]
        let frontmatter = fields
            .compactMap { key, value in value.map { "\(key): \($0)\n" } }
            .joined()
        let normalizedBody = body.hasSuffix("\n") ? body : body + "\n"
        return "---\n" + frontmatter + "---\n\n" + normalizedBody
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    /// A YAML double-quoted scalar. Quoting every string value keeps titles with
    /// colons/emoji, `sha256:` hashes, and ISO timestamps unambiguous for the
    /// core's YAML parser.
    private func quoted(_ string: String) -> String {
        var escaped = ""
        for character in string {
            switch character {
            case "\\": escaped += "\\\\"
            case "\"": escaped += "\\\""
            case "\n": escaped += "\\n"
            case "\t": escaped += "\\t"
            default: escaped.append(character)
            }
        }
        return "\"\(escaped)\""
    }

    private static let isoFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(identifier: "UTC")
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss.SSS'Z'"
        return formatter
    }()

    /// ISO-8601 UTC with millisecond precision — the format the core documents
    /// for `saved_at` / `read_at`, chosen so lexicographic order == chronological.
    static func timestamp(_ date: Date) -> String {
        isoFormatter.string(from: date)
    }

    static func sha256Hex(_ string: String) -> String {
        SHA256.hash(data: Data(string.utf8))
            .map { String(format: "%02x", $0) }
            .joined()
    }
}
