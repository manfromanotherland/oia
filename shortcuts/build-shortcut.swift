// SPDX-License-Identifier: MIT
// Builds the reviewable, unsigned source for the Óia! Shortcut.
// Run through `make shortcut`; the release script supplies the source fingerprint.
import Foundation

typealias Object = [String: Any]
guard CommandLine.arguments.count == 3 else {
    fatalError("Usage: swift shortcuts/build-shortcut.swift <destination> <source-sha256>")
}
let destination = CommandLine.arguments[1]
let sourceFingerprint = CommandLine.arguments[2]
guard sourceFingerprint.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil else {
    fatalError("The Shortcut source fingerprint must be a lowercase SHA-256 digest.")
}
var actions: [Object] = []
var sequence = 0

func identifier() -> String {
    sequence += 1
    return String(format: "C0771000-0000-4000-8000-%012d", sequence)
}

func variable(_ name: String) -> Object {
    ["Type": "Variable", "VariableName": name]
}

func attachment(_ value: Object) -> Object {
    ["Value": value, "WFSerializationType": "WFTextTokenAttachment"]
}

func text(_ parts: Any...) -> Object {
    var value = ""
    var ranges: Object = [:]
    for part in parts {
        if let token = part as? Object {
            ranges["{\(value.utf16.count), 1}"] = token
            value += "\u{fffc}"
        } else {
            value += String(describing: part)
        }
    }
    return ["Value": ["string": value, "attachmentsByRange": ranges],
            "WFSerializationType": "WFTextTokenString"]
}

@discardableResult
func action(_ name: String, _ parameters: Object = [:], uuid explicitUUID: String? = nil) -> Object {
    let uuid = explicitUUID ?? identifier()
    var parameters = parameters
    parameters["UUID"] = uuid
    actions.append(["WFWorkflowActionIdentifier": "is.workflow.actions.\(name)",
                    "WFWorkflowActionParameters": parameters])
    return ["Type": "ActionOutput", "OutputUUID": uuid, "OutputName": name]
}

func set(_ name: String, _ value: Object) {
    action("setvariable", ["WFVariableName": name, "WFInput": attachment(value)])
}

func field(_ name: String, _ value: Any, in dictionary: String = "Manifest") {
    let updated = action("setvalueforkey", ["WFDictionary": attachment(variable(dictionary)),
                                           "WFDictionaryKey": name,
                                           "WFDictionaryValue": text(value)])
    set(dictionary, updated)
}

func json(_ value: Object, named name: String) {
    let data = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    let string = action("gettext", ["WFTextActionText": String(data: data, encoding: .utf8)!])
    set(name, action("detect.dictionary", ["WFInput": attachment(string)]))
}

// Unlike ordinary action inputs, If wraps its variable in a typed subject.
// This representation round-trips through Apple's native action serializer.
func beginIf(_ subject: Object, equals value: String? = nil) -> String {
    let group = identifier()
    var parameters: Object = ["GroupingIdentifier": group, "WFControlFlowMode": 0,
                               "WFCondition": value == nil ? 100 : 4,
                               "WFInput": ["Type": "Variable", "Variable": attachment(subject)]]
    if let value { parameters["WFConditionalActionString"] = value }
    action("conditional", parameters)
    return group
}

func otherwise(_ group: String) {
    action("conditional", ["GroupingIdentifier": group, "WFControlFlowMode": 1])
}

func endIf(_ group: String) {
    action("conditional", ["GroupingIdentifier": group, "WFControlFlowMode": 2])
}

func setSaveConfirmation(_ value: Any) {
    let message = action("gettext", ["WFTextActionText": value])
    set("Save confirmation", message)
}

func setTextSaveConfirmation(_ input: Object) {
    let matched = action("text.match", ["text": text(input),
        "WFMatchTextPattern": "(?i)^\\s*https?://[^\\s]+\\s*$",
        "WFMatchTextCaseSensitive": false])
    let isLink = beginIf(matched)
    setSaveConfirmation("Link saved to Inbox")
    otherwise(isLink)
    setSaveConfirmation("Quote saved to Inbox")
    endIf(isLink)
}

// A version-2 request deliberately contains no ordinary link fallback. Older apps
// retain the unsupported archive rather than silently saving the wrong kind.
func beginInstagram(_ input: Object) -> String {
    let matched = action("text.match", ["text": text(input),
        "WFMatchTextPattern": "(?i)^https://(?:www\\.)?instagram\\.com/(?:p|reel|reels)/[^/?#]+/?(?:[?#].*)?$",
        "WFMatchTextCaseSensitive": false])
    let group = beginIf(matched)
    json(["version": 2], named: "Manifest")
    field("capture_id", variable("Capture ID"))
    field("captured_at", variable("Captured at"))
    field("instagram_url", input)
    setSaveConfirmation("Instagram media queued in Inbox")
    otherwise(group)
    return group
}

func captureMedia(_ input: Object, sourceURL: Object? = nil) {
    // Media and regular files keep their bytes; a safe fixed basename prevents
    // original filenames from colliding with the transport manifest.
    let originalFile = action("gettypeaction", ["WFInput": attachment(input), "WFFileType": "public.data"])
    json([:], named: "Origin")
    if let sourceURL { field("url", sourceURL, in: "Origin") }
    field("title", action("properties.files", ["WFInput": attachment(originalFile),
                                               "WFContentItemPropertyName": "Name"]), in: "Origin")
    let fileExtension = action("properties.files", ["WFInput": attachment(originalFile),
                                                    "WFContentItemPropertyName": "File Extension"])
    let payloadName = action("gettext", ["WFTextActionText": text("payload.", fileExtension)])
    let payload = action("setitemname", ["WFInput": attachment(originalFile),
                                         "WFName": text(payloadName), "WFDontIncludeFileExtension": false])
    action("appendvariable", ["WFVariableName": "Capture files", "WFInput": attachment(payload)])
    let digest = action("hash", ["WFInput": attachment(payload), "WFHashType": "SHA256"])
    // The filename is put into a Dictionary before JSON serialization; filenames
    // never get interpolated into JSON source.
    json([:], named: "Attachment")
    field("path", payloadName, in: "Attachment")
    field("sha256", digest, in: "Attachment")
    // Set Dictionary Value unwraps a one-item List into its single object. Parse
    // the array from JSON instead, then add the other fields without touching it.
    // Attachment itself is serialized by Shortcuts, so its values remain escaped.
    let mediaManifest = action("gettext", ["WFTextActionText": text(
        "{\"version\":3,\"capture_type\":\"media\",\"attachments\":[", variable("Attachment"), "]}")])
    set("Manifest", action("detect.dictionary", ["WFInput": attachment(mediaManifest)]))
    field("capture_id", variable("Capture ID"))
    field("captured_at", variable("Captured at"))
    field("origin", variable("Origin"))
}

let shortcutInput: Object = ["Type": "ExtensionInput"]
let repeatItem = variable("Repeat Item")
action("comment", ["WFCommentActionText": "Share an image, video, text or link to Óia! Choose the inbox folder inside your iCloud Óia library during setup. Each item is saved as a complete archive with its capture type; Óia imports it on your Mac. The shared Rust engine resolves links, categories and selected-text highlights. Direct image links download the image. Instagram posts queue the selected slide for download on your Mac. No accounts."])
let probeGroup = "0A1A0000-0000-4000-8000-000000000001"
action("conditional", ["GroupingIdentifier": probeGroup, "WFControlFlowMode": 0,
    "WFCondition": 101, "WFInput": ["Type": "Variable", "Variable": attachment(shortcutInput)]],
       uuid: "0A1A0000-0000-4000-8000-000000000002")
let probeResponse = action("gettext", ["WFTextActionText": "oia-shortcut/v2 sha256=\(sourceFingerprint)"],
                           uuid: "0A1A0000-0000-4000-8000-000000000003")
action("output", ["WFOutput": text(probeResponse)], uuid: "0A1A0000-0000-4000-8000-000000000004")
action("conditional", ["GroupingIdentifier": probeGroup, "WFControlFlowMode": 2],
       uuid: "0A1A0000-0000-4000-8000-000000000005")
action("comment", ["WFCommentActionText": "Official release marker: \(sourceFingerprint)"])
action("comment", ["WFCommentActionText": "Release verification runs only when the Shortcut has no input."])
let hasInput = beginIf(shortcutInput)
let repeatGroup = identifier()
action("repeat.each", ["GroupingIdentifier": repeatGroup, "WFControlFlowMode": 0,
                       "WFInput": attachment(shortcutInput)])
let now = action("date", ["WFDateActionMode": "Current Date"])
let date = action("format.date", ["WFDate": text(now), "WFDateFormatStyle": "ISO 8601",
                                  "WFISO8601IncludeTime": true])
set("Captured at", date)
let random = action("number.random", ["WFRandomNumberMinimum": 1, "WFRandomNumberMaximum": 999999999999])
let seed = action("gettext", ["WFTextActionText": text(date, "-", random, "-", variable("Repeat Index"))])
set("Capture ID", action("hash", ["WFInput": attachment(seed), "WFHashType": "SHA256"]))
json(["version": 3, "capture_type": "url"], named: "Manifest")
field("capture_id", variable("Capture ID"))
field("captured_at", variable("Captured at"))
set("Capture files", action("list", ["WFItems": [Any]()]))

let itemType = action("getitemtype", ["WFInput": attachment(repeatItem)])
let safari = beginIf(itemType, equals: "Safari Web Page")
setSaveConfirmation("Link saved to Inbox")
json([:], named: "Origin")
let pageURL = action("properties.safariwebpage", ["WFInput": attachment(repeatItem),
                                                "WFContentItemPropertyName": "Page URL"])
field("url", pageURL, in: "Origin")
field("title", action("properties.safariwebpage", ["WFInput": attachment(repeatItem),
                                                  "WFContentItemPropertyName": "Name"]), in: "Origin")
field("origin", variable("Origin"))
let selection = action("properties.safariwebpage", ["WFInput": attachment(repeatItem),
                                                   "WFContentItemPropertyName": "Page Selection"])
// A whole-page share has no selection. Set Dictionary Value rejects a missing
// value on iOS; omit the optional text field so this remains a lightweight link.
let hasSelection = beginIf(selection)
setSaveConfirmation("Selection saved to Inbox")
field("capture_type", "selection")
field("text", selection)
otherwise(hasSelection)
let safariInstagram = beginInstagram(pageURL)
endIf(safariInstagram)
endIf(hasSelection)
otherwise(safari)
let url = beginIf(itemType, equals: "URL")
setSaveConfirmation("Link saved to Inbox")
let instagram = beginInstagram(repeatItem)
// Safari's long-press image share can supply only a direct image URL.
// Match the URL path, never a filename embedded in an ordinary page's query.
let imageURL = action("text.match", ["text": text(repeatItem),
    "WFMatchTextPattern": "(?i)^https?://[^/?#]+/[^?#]*\\.(?:jpe?g|png|gif|webp|heic|heif)(?:[?#].*)?$",
    "WFMatchTextCaseSensitive": false])
let isImageURL = beginIf(imageURL)
let downloaded = action("downloadurl", ["WFURL": text(repeatItem), "WFHTTPMethod": "GET"])
let downloadedType = action("getitemtype", ["WFInput": attachment(downloaded)])
let isImage = beginIf(downloadedType, equals: "Image")
captureMedia(downloaded, sourceURL: repeatItem)
setSaveConfirmation("Image saved to Inbox")
otherwise(isImage)
action("alert", ["WFAlertActionTitle": "Image could not be saved",
    "WFAlertActionMessage": "The shared image URL did not return an image. Try sharing the image file instead.",
    "WFAlertActionCancelButtonShown": false])
action("exit")
endIf(isImage)
otherwise(isImageURL)
json([:], named: "Origin")
field("url", repeatItem, in: "Origin")
field("origin", variable("Origin"))
endIf(isImageURL)
endIf(instagram)
otherwise(url)
let plain = beginIf(itemType, equals: "Text")
let textInstagram = beginInstagram(repeatItem)
setTextSaveConfirmation(repeatItem)
field("capture_type", "text")
field("text", repeatItem)
endIf(textInstagram)
otherwise(plain)
let rich = beginIf(itemType, equals: "Rich Text")
let richText = action("detect.text", ["WFInput": attachment(repeatItem)])
let richInstagram = beginInstagram(richText)
setTextSaveConfirmation(richText)
field("capture_type", "text")
field("text", richText)
endIf(richInstagram)
otherwise(rich)

setSaveConfirmation(text(itemType, " saved to Inbox"))
captureMedia(repeatItem)
endIf(rich)
endIf(plain)
endIf(url)
endIf(safari)

let manifestText = action("gettext", ["WFTextActionText": text(variable("Manifest"))])
let manifestFile = action("setitemname", ["WFInput": attachment(manifestText),
                                          "WFName": "manifest.json", "WFDontIncludeFileExtension": false])
action("appendvariable", ["WFVariableName": "Capture files", "WFInput": attachment(manifestFile)])
let archive = action("makezip", ["WFInput": attachment(variable("Capture files")),
                                 "WFArchiveFormat": "zip"])
// Make Archive's name field is ignored in some native execution paths. Rename
// the resulting file explicitly, as we do for manifest.json and payload files.
let captureFile = action("setitemname", ["WFInput": attachment(archive),
    "WFName": text(variable("Capture ID"), ".cuttingscapture.zip"),
    "WFDontIncludeFileExtension": false])
let saveActionIndex = actions.count
action("documentpicker.save", ["WFInput": attachment(captureFile), "WFAskWhereToSave": false,
                                "WFSaveFileOverwrite": false])
action("repeat.each", ["GroupingIdentifier": repeatGroup, "WFControlFlowMode": 2])
let inputCount = action("count", ["Input": attachment(shortcutInput), "WFCountType": "Items"])
let inputCountText = action("gettext", ["WFTextActionText": text(inputCount)])
let singleSave = beginIf(inputCountText, equals: "1")
set("Notification copy", variable("Save confirmation"))
otherwise(singleSave)
let pluralConfirmation = action("gettext", ["WFTextActionText": text(inputCount, " items saved to Inbox")])
set("Notification copy", pluralConfirmation)
endIf(singleSave)
action("notification", ["WFNotificationActionTitle": text(variable("Notification copy")),
                         "WFNotificationActionSound": false])
otherwise(hasInput)
action("alert", ["WFAlertActionTitle": "Óia!",
                  "WFAlertActionMessage": "Open an image, video, text or link. Tap Share, then choose Óia!",
                  "WFAlertActionCancelButtonShown": false])
endIf(hasInput)

let workflow: Object = [
    "WFWorkflowName": "Óia!",
    "OiaShortcutProbeVersion": 2,
    "OiaShortcutFingerprint": sourceFingerprint,
    "WFWorkflowClientVersion": "2302.0.4",
    "WFWorkflowMinimumClientVersion": 900,
    "WFWorkflowMinimumClientVersionString": "900",
    "WFWorkflowIcon": ["WFWorkflowIconStartColor": 255, "WFWorkflowIconGlyphNumber": 59745],
    "WFWorkflowTypes": ["ActionExtension"],
    "WFQuickActionSurfaces": [String](),
    "WFWorkflowHasShortcutInputVariables": true,
    "WFWorkflowHasOutputFallback": false,
    "WFWorkflowInputContentItemClasses": ["WFSafariWebPageContentItem", "WFURLContentItem",
        "WFStringContentItem", "WFRichTextContentItem", "WFImageContentItem", "WFAVAssetContentItem",
        "WFGenericFileContentItem"],
    "WFWorkflowOutputContentItemClasses": [String](),
    "WFWorkflowImportQuestions": [["ActionIndex": saveActionIndex, "Category": "Parameter",
        "ParameterKey": "WFFolder", "Text": "Choose the inbox folder inside your iCloud Óia library."]],
    "WFWorkflowActions": actions,
]
let bytes = try PropertyListSerialization.data(fromPropertyList: workflow, format: .xml, options: 0)
try bytes.write(to: URL(fileURLWithPath: destination), options: .atomic)
print("Built \(actions.count) actions: \(destination)")
