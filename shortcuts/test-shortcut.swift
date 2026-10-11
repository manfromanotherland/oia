// SPDX-License-Identifier: MIT
// Regression checks against the generated Safari action graph, not a second
// implementation of its capture logic. This does not execute Apple Shortcuts;
// native serialization validation and an iPhone Share Sheet rerun remain needed.
// Run: swift shortcuts/test-shortcut.swift <unsigned.shortcut>
import Foundation

typealias Object = [String: Any]
struct Failure: Error, CustomStringConvertible { let description: String }
func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw Failure(description: message) }
}
guard CommandLine.arguments.count == 2 else {
    fatalError("Usage: swift shortcuts/test-shortcut.swift <unsigned.shortcut>")
}
let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let workflow = try PropertyListSerialization.propertyList(from: data, format: nil) as! Object
let actions = workflow["WFWorkflowActions"] as! [Object]
func parameters(_ action: Object) -> Object { action["WFWorkflowActionParameters"] as! Object }
func kind(_ action: Object) -> String {
    String((action["WFWorkflowActionIdentifier"] as! String).dropFirst("is.workflow.actions.".count))
}
func containsVariable(_ value: Any, named name: String) -> Bool {
    if let object = value as? Object {
        if object["Type"] as? String == "Variable", object["VariableName"] as? String == name {
            return true
        }
        return object.values.contains { containsVariable($0, named: name) }
    }
    if let array = value as? [Any] {
        return array.contains { containsVariable($0, named: name) }
    }
    return false
}
func containsOutput(_ value: Any, uuid: String) -> Bool {
    if let object = value as? Object {
        if object["Type"] as? String == "ActionOutput", object["OutputUUID"] as? String == uuid {
            return true
        }
        return object.values.contains { containsOutput($0, uuid: uuid) }
    }
    if let array = value as? [Any] {
        return array.contains { containsOutput($0, uuid: uuid) }
    }
    return false
}

// Evaluate the per-item capture branches. ZIP creation and saving are out of scope.
let start = try actions.firstIndex { action in
    guard kind(action) == "gettext", let text = parameters(action)["WFTextActionText"] as? String,
          let bytes = text.data(using: .utf8),
          let dictionary = try? JSONSerialization.jsonObject(with: bytes) as? Object else { return false }
    return dictionary.count == 2 && dictionary["version"] as? Int == 3
        && dictionary["capture_type"] as? String == "url"
}.unwrap("Missing per-item manifest initialization")
let safariIf = try actions.firstIndex {
    kind($0) == "conditional" && parameters($0)["WFConditionalActionString"] as? String == "Safari Web Page"
}.unwrap("Missing Safari branch")
let safariGroup = parameters(actions[safariIf])["GroupingIdentifier"] as! String
let end = try actions.indices.first {
    $0 > safariIf && kind(actions[$0]) == "conditional"
        && parameters(actions[$0])["GroupingIdentifier"] as? String == safariGroup
        && parameters(actions[$0])["WFControlFlowMode"] as? Int == 2
}.unwrap("Missing Safari branch boundary")
try require(start < safariIf, "Manifest initialization must precede the Safari branch")
let graph = Array(actions[start...end])

extension Optional {
    func unwrap(_ message: String) throws -> Wrapped {
        guard let value = self else { throw Failure(description: message) }
        return value
    }
}

struct SafariFixture {
    let url: String
    let title: String
    let selection: String?
    var type = "Safari Web Page"
    var responseType = "Image"
}

struct SafariGraph {
    var variables: Object = [:]
    var outputs: Object = [:]
    var downloads: [String] = []
    var payloads: [String] = []

    func resolve(_ value: Any?) throws -> Any? {
        guard let value else { return nil }
        guard let object = value as? Object else { return value }
        if let serialization = object["WFSerializationType"] as? String {
            if serialization == "WFTextTokenAttachment" { return try resolve(object["Value"]) }
            try require(serialization == "WFTextTokenString", "Unsupported token serialization: \(serialization)")
            let body = object["Value"] as! Object
            let string = body["string"] as! String
            let attachments = body["attachmentsByRange"] as! Object
            // A lone variable carries its typed dictionary (or absent value).
            if string == "\u{fffc}", attachments.count == 1 {
                return try resolve(attachments.values.first)
            }
            let rendered = NSMutableString(string: string)
            for (range, token) in attachments.sorted(by: { NSRangeFromString($0.key).location > NSRangeFromString($1.key).location }) {
                let value = try resolve(token)
                let replacement: String
                if let dictionary = value as? Object {
                    replacement = String(data: try JSONSerialization.data(withJSONObject: dictionary, options: [.sortedKeys]), encoding: .utf8)!
                } else { replacement = value.map { String(describing: $0) } ?? "" }
                rendered.replaceCharacters(in: NSRangeFromString(range), with: replacement)
            }
            return rendered as String
        }
        switch object["Type"] as? String {
        case "ActionOutput": return outputs[object["OutputUUID"] as! String]
        case "Variable":
            if let subject = object["Variable"] { return try resolve(subject) }
            return variables[object["VariableName"] as! String]
        default: throw Failure(description: "Unsupported variable in Safari graph: \(object)")
        }
    }

    func hasValue(_ value: Any?) -> Bool {
        guard let value else { return false }
        if let string = value as? String { return !string.isEmpty }
        if let list = value as? [Any] { return !list.isEmpty }
        return true
    }

    mutating func capture(_ fixture: SafariFixture, index: Int) throws -> Object {
        variables["Capture ID"] = "fixture-\(index)"
        variables["Captured at"] = "2026-09-18T12:00:00Z"
        variables["Repeat Item"] = fixture.url
        var conditions: [(group: String, parent: Bool, matches: Bool)] = []
        var active = true
        for action in graph {
            let p = parameters(action)
            if kind(action) == "conditional" {
                let group = p["GroupingIdentifier"] as! String
                switch p["WFControlFlowMode"] as! Int {
                case 0:
                    let subject = active ? try resolve(p["WFInput"]) : nil
                    let operation = p["WFCondition"] as! Int
                    try require(operation == 100 || operation == 4, "Unsupported If operation")
                    let matches = operation == 100 ? hasValue(subject)
                        : subject as? String == p["WFConditionalActionString"] as? String
                    conditions.append((group, active, matches))
                    active = active && matches
                case 1:
                    let condition = try conditions.last.unwrap("Unexpected Otherwise")
                    try require(condition.group == group, "Mismatched Otherwise")
                    active = condition.parent && !condition.matches
                case 2:
                    let condition = try conditions.popLast().unwrap("Unexpected End If")
                    try require(condition.group == group, "Mismatched End If")
                    active = condition.parent
                default: throw Failure(description: "Unsupported control-flow mode")
                }
                continue
            }
            guard active else { continue }
            var output: Any?
            switch kind(action) {
            case "gettext": output = try resolve(p["WFTextActionText"])
            case "detect.dictionary":
                let text = try (resolve(p["WFInput"]) as? String).unwrap("Dictionary input is not JSON text")
                output = try JSONSerialization.jsonObject(with: Data(text.utf8))
            case "setvariable": variables[p["WFVariableName"] as! String] = try resolve(p["WFInput"])
            case "setvalueforkey":
                var dictionary = try (resolve(p["WFDictionary"]) as? Object).unwrap("Missing dictionary")
                let key = p["WFDictionaryKey"] as! String
                let value = try resolve(p["WFDictionaryValue"])
                // This failure contract is the user's captured iOS error.
                try require(hasValue(value), "No Value Provided. No value was provided to the Set Dictionary Value action for the key “\(key)”.")
                dictionary[key] = value
                output = dictionary
            case "list": output = p["WFItems"] as! [Any]
            case "getitemtype":
                let input = try resolve(p["WFInput"]) as? String
                output = input == "downloaded-image" ? fixture.responseType : fixture.type
            case "text.match":
                let input = try resolve(p["text"]) as! String
                let regex = try NSRegularExpression(pattern: p["WFMatchTextPattern"] as! String)
                output = regex.matches(in: input, range: NSRange(input.startIndex..., in: input)).map { (input as NSString).substring(with: $0.range) }
            case "downloadurl":
                downloads.append(try resolve(p["WFURL"]) as! String)
                output = "downloaded-image"
            case "gettypeaction": output = try resolve(p["WFInput"])
            case "properties.files": output = p["WFContentItemPropertyName"] as? String == "Name" ? "shared.jpg" : "jpg"
            case "setitemname": output = try resolve(p["WFName"])
            case "appendvariable": payloads.append(try resolve(p["WFInput"]) as! String)
            case "hash": output = String(repeating: "a", count: 64)
            case "detect.text": output = try resolve(p["WFInput"])
            case "alert": break
            case "exit": throw Failure(description: "Capture stopped")
            case "properties.safariwebpage":
                switch p["WFContentItemPropertyName"] as! String {
                case "Page URL": output = fixture.url
                case "Name": output = fixture.title
                case "Page Selection": output = fixture.selection
                default: throw Failure(description: "Unexpected Safari property")
                }
            default: throw Failure(description: "Unexpected action in Safari graph: \(kind(action))")
            }
            // Clear absent outputs as well, so an earlier item's selection
            // cannot stand in for the current Page Selection result.
            outputs[p["UUID"] as! String] = output
        }
        try require(conditions.isEmpty,
                    "Safari branch did not finish its nested conditions")
        return try (variables["Manifest"] as? Object).unwrap("Missing manifest")
    }
}

struct NotificationGraph {
    let graph: [Object]
    let shortcutInput: [String]
    var variables: Object = [:]
    var outputs: Object = [:]

    func resolve(_ value: Any?) throws -> Any? {
        guard let value else { return nil }
        guard let object = value as? Object else { return value }
        if let serialization = object["WFSerializationType"] as? String {
            if serialization == "WFTextTokenAttachment" { return try resolve(object["Value"]) }
            try require(serialization == "WFTextTokenString", "Unsupported summary token serialization: \(serialization)")
            let body = object["Value"] as! Object
            let string = body["string"] as! String
            let attachments = body["attachmentsByRange"] as! Object
            let rendered = NSMutableString(string: string)
            for (range, token) in attachments.sorted(by: { NSRangeFromString($0.key).location > NSRangeFromString($1.key).location }) {
                let replacement = try resolve(token).map { String(describing: $0) } ?? ""
                rendered.replaceCharacters(in: NSRangeFromString(range), with: replacement)
            }
            return rendered as String
        }
        switch object["Type"] as? String {
        case "ActionOutput": return outputs[object["OutputUUID"] as! String]
        case "Variable":
            if let subject = object["Variable"] { return try resolve(subject) }
            return variables[object["VariableName"] as! String]
        case "ExtensionInput": return shortcutInput
        default: throw Failure(description: "Unsupported variable in notification graph: \(object)")
        }
    }

    mutating func title(saveConfirmation: String) throws -> String {
        variables["Save confirmation"] = saveConfirmation
        var conditions: [(group: String, parent: Bool, matches: Bool)] = []
        var active = true
        for action in graph {
            let p = parameters(action)
            if kind(action) == "conditional" {
                let group = p["GroupingIdentifier"] as! String
                switch p["WFControlFlowMode"] as! Int {
                case 0:
                    let subject = active ? try resolve(p["WFInput"]) as? String : nil
                    let matches = subject == p["WFConditionalActionString"] as? String
                    conditions.append((group, active, matches))
                    active = active && matches
                case 1:
                    let condition = try conditions.last.unwrap("Unexpected summary Otherwise")
                    try require(condition.group == group, "Mismatched summary Otherwise")
                    active = condition.parent && !condition.matches
                case 2:
                    let condition = try conditions.popLast().unwrap("Unexpected summary End If")
                    try require(condition.group == group, "Mismatched summary End If")
                    active = condition.parent
                default: throw Failure(description: "Unsupported summary control-flow mode")
                }
                continue
            }
            guard active else { continue }
            var output: Any?
            switch kind(action) {
            case "count":
                let input = try (resolve(p["Input"]) as? [Any]).unwrap("Count does not use Shortcut Input")
                output = input.count
            case "gettext": output = try resolve(p["WFTextActionText"])
            case "setvariable": variables[p["WFVariableName"] as! String] = try resolve(p["WFInput"])
            case "notification":
                try require(conditions.isEmpty, "Notification summary has unclosed control flow")
                return try (resolve(p["WFNotificationActionTitle"]) as? String).unwrap("Missing notification title")
            default: throw Failure(description: "Unexpected action in notification graph: \(kind(action))")
            }
            outputs[p["UUID"] as! String] = output
        }
        throw Failure(description: "Notification graph did not show a notification")
    }
}

let selectedText = "A \"quoted\" selection\nwith café and 🌱"
let cases: [(String, [String?])] = [
    ("whole page: absent selection", [nil]),
    ("whole page: empty selection", [""]),
    ("selected text preserved", [selectedText]),
    ("selected URL remains a selection", ["https://example.com/selected-link"]),
    ("repeat items do not retain prior selection", [selectedText, nil, "", "A different selection"]),
]
var failures = 0
for (name, selections) in cases {
    do {
        var runner = SafariGraph()
        for (index, selection) in selections.enumerated() {
            let fixture = SafariFixture(url: "https://example.com/page/\(index)",
                                        title: "Page \(index)", selection: selection)
            let manifest = try runner.capture(fixture, index: index)
            let origin = manifest["origin"] as? Object
            try require(origin?["url"] as? String == fixture.url, "Source URL was lost or stale")
            try require(origin?["title"] as? String == fixture.title, "Source title was lost or stale")
            try require(manifest["version"] as? Int == 3 && manifest["capture_id"] as? String == "fixture-\(index)"
                        && manifest["captured_at"] as? String == "2026-09-18T12:00:00Z", "Capture metadata was lost or stale")
            if let selection, !selection.isEmpty {
                try require(manifest["capture_type"] as? String == "selection", "Safari selection lost its explicit capture type")
                try require(manifest["text"] as? String == selection, "Selected quote text was lost or changed")
                try require(runner.variables["Save confirmation"] as? String == "Selection saved to Inbox",
                            "Selected text confirmation is not contextual")
            } else {
                try require(manifest["capture_type"] as? String == "url", "Whole-page share retained a prior selection type")
                try require(manifest["text"] == nil, "Whole-page link retained a text field")
                try require(runner.variables["Save confirmation"] as? String == "Link saved to Inbox",
                            "Whole-page link confirmation is not contextual")
            }
        }
        print("PASS: \(name)")
    } catch {
        failures += 1
        print("FAIL: \(name): \(error)")
    }
}
do {
    try require(workflow["OiaShortcutProbeVersion"] as? Int == 2,
                "Shortcut version probe contract is missing")
    try require(!Set([140, 180, 220, 224, 225]).contains(actions.count),
                "Probe-enabled releases must not reuse a known unsafe action count")
    let fingerprint = try (workflow["OiaShortcutFingerprint"] as? String)
        .unwrap("Missing Shortcut release fingerprint")
    try require(fingerprint.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil,
                "Shortcut release fingerprint must be a lowercase SHA-256 digest")
    let probeStart = try actions.firstIndex {
        kind($0) == "conditional" && parameters($0)["WFCondition"] as? Int == 101
    }.unwrap("Missing installed-version probe")
    try require(probeStart == 1,
                "Installed-version probe must run before every capture action")
    try require(parameters(actions[probeStart])["WFConditionalActionString"] == nil,
                "No-input probe must not depend on a caller-provided sentinel")
    let probeGroup = parameters(actions[probeStart])["GroupingIdentifier"] as! String
    let probeEnd = try actions.indices.first {
        $0 > probeStart && kind(actions[$0]) == "conditional"
            && parameters(actions[$0])["GroupingIdentifier"] as? String == probeGroup
            && parameters(actions[$0])["WFControlFlowMode"] as? Int == 2
    }.unwrap("Missing installed-version probe boundary")
    let probe = Array(actions[probeStart...probeEnd])
    try require(probe.map(kind) == ["conditional", "gettext", "output", "conditional"],
                "Installed-version probe must return before any capture side effect")
    let response = parameters(probe[1])
    try require(response["WFTextActionText"] as? String == "oia-shortcut/v2 sha256=\(fingerprint)",
                "Installed-version probe response does not match the release fingerprint")
    let responseUUID = response["UUID"] as! String
    try require(containsOutput(parameters(probe[2])["WFOutput"] as Any, uuid: responseUUID),
                "Installed-version probe must return its fingerprint")
    print("PASS: installed-version probe returns before capture side effects")
} catch { failures += 1; print("FAIL: installed-version probe: \(error)") }
do {
    for type in ["URL", "Text", "Rich Text", "Safari Web Page"] {
        var runner = SafariGraph()
        let url = "https://www.instagram.com/p/DdlVpikk5Gj/?img_index=2&stkn=tracking"
        let manifest = try runner.capture(SafariFixture(url: url, title: "Post", selection: nil, type: type), index: 0)
        try require(manifest["version"] as? Int == 2 && manifest["instagram_url"] as? String == url,
                    "Instagram slide must survive as an explicit version-2 request: \(type)")
        try require(manifest["capture_type"] == nil && manifest["text"] == nil && manifest["origin"] == nil && manifest["attachments"] == nil && runner.downloads.isEmpty,
                    "Instagram must not download on iPhone or silently become a link")
        try require(runner.variables["Save confirmation"] as? String == "Instagram media queued in Inbox",
                    "Instagram confirmation must describe the queued handoff")
    }
    print("PASS: Instagram URL, plain/rich text and Safari shares queue the selected slide")
} catch { failures += 1; print("FAIL: Instagram request: \(error)") }
let urlCases: [(String, Bool)] = [
    ("https://media.houseandgarden.co.uk/photos/67879b979514423c41c6e4ea/master/w_1280,c_limit/11-13-24-HG-Anna-Hambro011.jpg", true),
    ("https://example.com/photo.PNG?width=1200#image", true),
    ("https://example.com/photo.webp", true),
    ("https://example.com/article", false),
    ("https://example.com/article?image=photo.jpg", false),
    ("https://example.com/photo.jpg/article", false),
    ("file:///tmp/photo.jpg", false),
]
do {
    var runner = SafariGraph()
    for (index, entry) in urlCases.enumerated() {
        let (url, isImage) = entry
        runner.downloads = []
        runner.payloads = []
        let manifest = try runner.capture(SafariFixture(url: url, title: "", selection: nil, type: "URL"), index: index)
        try require(manifest["version"] as? Int == 3 && manifest["capture_type"] as? String == (isImage ? "media" : "url"),
                    "URL branch did not describe its actual payload type: \(url)")
        try require((manifest["origin"] as? Object)?["url"] as? String == url, "Shared URL was lost")
        try require(runner.downloads == (isImage ? [url] : []), "Wrong download decision: \(url)")
        try require(runner.payloads == (isImage ? ["payload.jpg"] : []), "Image bytes were not attached: \(url)")
        if isImage {
            let attachments = manifest["attachments"] as? [Object]
            try require(attachments?.count == 1 && attachments?.first?["path"] as? String == "payload.jpg", "Missing image attachment array")
            try require(attachments?.first?["sha256"] as? String == String(repeating: "a", count: 64), "Missing image checksum")
        } else { try require(manifest["attachments"] == nil, "Page URL retained a prior image") }
        let confirmation = isImage ? "Image saved to Inbox" : "Link saved to Inbox"
        try require(runner.variables["Save confirmation"] as? String == confirmation,
                    "URL confirmation did not match its saved kind: \(url)")
    }
    print("PASS: direct image URL and ordinary link capture (including House & Garden)")
    var rejected = false
    do {
        _ = try runner.capture(SafariFixture(url: "https://example.com/error.jpg", title: "", selection: nil, type: "URL", responseType: "Text"), index: 0)
    } catch { rejected = String(describing: error) == "Capture stopped" }
    try require(rejected, "A non-image response must stop instead of saving a link")
    print("PASS: non-image response stops capture")
    runner.downloads = []
    let local = try runner.capture(SafariFixture(url: "local-image", title: "", selection: nil, type: "Image"), index: 0)
    try require(local["version"] as? Int == 3 && local["capture_type"] as? String == "media", "Local image lost its explicit media type")
    try require((local["attachments"] as? [Object])?.count == 1 && runner.downloads.isEmpty, "Local image capture regressed")
    try require(runner.variables["Save confirmation"] as? String == "Image saved to Inbox",
                "Local image confirmation is not contextual")
    print("PASS: local images retain attachments without downloading")
    let plain = try runner.capture(SafariFixture(url: "A thought worth keeping", title: "", selection: nil, type: "Text"), index: 0)
    try require(plain["version"] as? Int == 3 && plain["capture_type"] as? String == "text", "Plain text lost its explicit text type")
    try require(runner.variables["Save confirmation"] as? String == "Quote saved to Inbox",
                "Plain text confirmation is not contextual")
    let rich = try runner.capture(SafariFixture(url: "https://example.com/from-text", title: "", selection: nil, type: "Rich Text"), index: 0)
    try require(rich["version"] as? Int == 3 && rich["capture_type"] as? String == "text", "Rich text URL must preserve its transport type for Rust to resolve")
    try require(runner.variables["Save confirmation"] as? String == "Link saved to Inbox",
                "A URL supplied as text must use the link confirmation")
    let media = try runner.capture(SafariFixture(url: "local-video", title: "", selection: nil, type: "Media"), index: 0)
    try require(media["version"] as? Int == 3 && media["capture_type"] as? String == "media", "Video lost its explicit media type")
    try require(runner.variables["Save confirmation"] as? String == "Media saved to Inbox",
                "Media confirmation is not contextual")
    print("PASS: text and media use contextual confirmations")
    try require(workflow["WFWorkflowName"] as? String == "Óia!", "Shortcut name is stale")
    let notifications = actions.filter { kind($0) == "notification" }
    try require(notifications.count == 1, "The Shortcut must show one summary notification")
    let notification = parameters(notifications[0])
    try require(notification["WFNotificationActionBody"] == nil,
                "The summary must not add a redundant notification body")
    try require(containsVariable(notification["WFNotificationActionTitle"] as Any, named: "Notification copy"),
                "The notification title must use the contextual summary")
    let countIndex = try actions.firstIndex { kind($0) == "count" && parameters($0)["WFCountType"] as? String == "Items" }
        .unwrap("Multi-item shares must count their saved items")
    let notificationIndex = try actions.firstIndex { kind($0) == "notification" }
        .unwrap("Missing summary notification")
    try require(countIndex < notificationIndex, "Item count must precede the summary notification")
    let summaryGraph = Array(actions[countIndex...notificationIndex])
    var singleSummary = NotificationGraph(graph: summaryGraph, shortcutInput: ["image"])
    try require(singleSummary.title(saveConfirmation: "Image saved to Inbox") == "Image saved to Inbox",
                "Single-item summary lost its contextual confirmation")
    var pluralSummary = NotificationGraph(graph: summaryGraph, shortcutInput: ["image", "link", "quote"])
    try require(pluralSummary.title(saveConfirmation: "Quote saved to Inbox") == "3 items saved to Inbox",
                "Multi-item summary did not use its item count")
    print("PASS: notification has one contextual title and no duplicate Óia line")
} catch {
    failures += 1
    print("FAIL: Shortcut regression: \(error)")
}
print("Action-graph regression checks: \(failures) failures (\(actions.count)-action workflow).")
print("This models missing-value/control-flow semantics; it is not a native or iPhone execution test.")
exit(failures == 0 ? 0 : 1)
