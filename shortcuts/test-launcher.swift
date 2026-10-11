// SPDX-License-Identifier: MIT
// Checks the permanent launcher contract, including real exported migrations.
import Foundation

typealias Object = [String: Any]
func load(_ path: String) throws -> Object {
    try PropertyListSerialization.propertyList(from: Data(contentsOf: URL(fileURLWithPath: path)), format: nil) as! Object
}
func check(_ condition: @autoclosure () -> Bool, _ message: String) {
    guard condition() else { fputs("FAIL: \(message)\n", stderr); exit(1) }
}
guard (2...3).contains(CommandLine.arguments.count) else {
    fatalError("Usage: swift shortcuts/test-launcher.swift <launcher.wflow> [pre-migration.wflow]")
}
let workflow = try load(CommandLine.arguments[1])
let actions = workflow["WFWorkflowActions"] as! [Object]
let kinds = actions.map { $0["WFWorkflowActionIdentifier"] as! String }
check(kinds == ["gettext", "runworkflow", "count", "math", "getitemfromlist", "documentpicker.save", "getitemfromlist", "notification"].map { "is.workflow.actions." + $0 }, "Unexpected launcher actions or ordering")
let p = actions.map { $0["WFWorkflowActionParameters"] as! Object }
func output(_ value: Any?, from index: Int) -> Bool {
    guard let token = value as? Object, let inner = token["Value"] as? Object else { return false }
    return inner["Type"] as? String == "ActionOutput" && inner["OutputUUID"] as? String == p[index]["UUID"] as? String
}
check((p[0]["WFTextActionText"] as? String)?.precomposedStringWithCanonicalMapping == "Óia Capture", "Helper name changed")
check(output(p[1]["WFWorkflow"], from: 0), "Helper must be resolved by name rather than a replaced shortcut ID")
check(((p[1]["WFInput"] as? Object)?["Value"] as? Object)?["Type"] as? String == "ExtensionInput", "Original Share Sheet input is not passed to the helper")
check(output(p[2]["Input"], from: 1), "Count must read helper output")
check(output(p[3]["WFInput"], from: 2) && p[3]["WFMathOperation"] as? String == "-" && p[3]["WFMathOperand"] as? String == "1", "Archive count must exclude exactly one confirmation item")
check(output(p[4]["WFInput"], from: 1) && output(p[4]["WFItemRangeEnd"], from: 3) && p[4]["WFItemRangeStart"] as? String == "1" && p[4]["WFItemSpecifier"] as? String == "Items in Range", "Archive range is incorrect")
check(output(p[5]["WFInput"], from: 4) && p[5]["WFAskWhereToSave"] as? Bool == false && p[5]["WFSaveFileOverwrite"] as? Bool == false, "Save File must receive only archives and retain destination/overwrite settings")
check(output(p[6]["WFInput"], from: 1) && p[6]["WFItemSpecifier"] as? String == "Last Item", "Confirmation must be the final helper output")
let body = ((p[7]["WFNotificationActionBody"] as? Object)?["Value"] as? Object)?["attachmentsByRange"] as? Object
let token = body?["{0, 1}"] as? Object
check(token?["OutputUUID"] as? String == p[6]["UUID"] as? String && p[7]["WFNotificationActionSound"] as? Bool == false, "Notification must use the returned confirmation after saving")
check(((workflow["WFWorkflowNoInputBehavior"] as? Object)?["Name"] as? String) == "WFWorkflowNoInputBehaviorShowError", "Empty input must stop before calling the helper")
check((workflow["WFWorkflowTypes"] as? [String])?.contains("ActionExtension") == true, "Permanent launcher must remain in the Share Sheet")
if CommandLine.arguments.count == 3 {
    let before = try load(CommandLine.arguments[2])
    let original = (before["WFWorkflowActions"] as! [Object]).first { $0["WFWorkflowActionIdentifier"] as? String == "is.workflow.actions.documentpicker.save" }!["WFWorkflowActionParameters"] as! Object
    check(NSDictionary(dictionary: original["WFFolder"] as! Object) == NSDictionary(dictionary: p[5]["WFFolder"] as! Object), "Configured Inbox reference changed during migration")
    check(NSDictionary(dictionary: workflow["WFWorkflowIcon"] as! Object) == NSDictionary(dictionary: before["WFWorkflowIcon"] as! Object), "Shortcut appearance changed during migration")
    check((workflow["WFWorkflowInputContentItemClasses"] as! [String]) == (before["WFWorkflowInputContentItemClasses"] as! [String]), "Accepted Share Sheet input types changed")
    print("PASS: original Inbox bookmark, red heart and accepted inputs are unchanged")
}
print("PASS: permanent launcher calls helper by name, saves only archives, and confirms after saving")
