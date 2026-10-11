// SPDX-License-Identifier: MIT
// Local developer validation against Apple's installed action definitions.
// This does not execute, install, or share a Shortcut.
import Foundation
import ObjectiveC

guard CommandLine.arguments.count == 2 else {
    fatalError("Usage: swift shortcuts/validate-shortcut.swift <unsigned.shortcut>")
}
guard Bundle(path: "/System/Library/PrivateFrameworks/WorkflowKit.framework")?.load() == true,
      Bundle(path: "/System/Library/PrivateFrameworks/ActionKit.framework")?.load() == true,
      let registryClass = NSClassFromString("WFActionRegistry") as? NSObject.Type,
      let registry = registryClass.perform(NSSelectorFromString("sharedRegistry"))?.takeUnretainedValue() as? NSObject
else { fatalError("Apple's local Shortcuts action registry is unavailable.") }

let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let workflow = try PropertyListSerialization.propertyList(from: data, format: nil) as! [String: Any]
let actions = workflow["WFWorkflowActions"] as! [[String: Any]]
let isLauncher = workflow["WFWorkflowName"] as? String == "Óia!"
var errors: [String] = []
var groups: [String] = []
var uuids = Set<String>()
func variableKeys(in value: Any) -> Set<String> {
    if let object = value as? [String: Any] {
        if object["Type"] as? String == "ActionOutput", let id = object["OutputUUID"] as? String { return ["output:\(id)"] }
        if object["Type"] as? String == "Variable", let name = object["VariableName"] as? String { return ["variable:\(name)"] }
        if object["Type"] as? String == "ExtensionInput" { return ["input"] }
        return object.values.reduce(into: Set<String>()) { $0.formUnion(variableKeys(in: $1)) }
    }
    if let array = value as? [Any] { return array.reduce(into: Set<String>()) { $0.formUnion(variableKeys(in: $1)) } }
    return []
}
for (index, serialized) in actions.enumerated() {
    let identifier = serialized["WFWorkflowActionIdentifier"] as! String
    let parameters = serialized["WFWorkflowActionParameters"] as! [String: Any]
    guard let action = registry.perform(NSSelectorFromString("createActionWithIdentifier:serializedParameters:"),
                                        with: identifier, with: parameters)?.takeUnretainedValue() as? NSObject,
          (action.value(forKey: "isMissing") as? Bool) == false else {
        errors.append("Action \(index): unknown action \(identifier)")
        continue
    }
    let definitions = action.value(forKey: "parameterDefinitions") as? [NSObject] ?? []
    let keys = Set(definitions.compactMap {
        $0.perform(NSSelectorFromString("objectForKey:"), with: "Key")?.takeUnretainedValue() as? String
    })
    let structural: Set<String> = ["UUID", "GroupingIdentifier", "WFControlFlowMode", "CustomOutputName"]
    let legacyWorkflow: Set<String> = identifier == "is.workflow.actions.runworkflow" ? ["WFWorkflowName"] : []
    // Single-row If conditions use the legacy keys when serialized by Apple.
    let legacyIf: Set<String> = identifier == "is.workflow.actions.conditional"
        ? ["WFCondition", "WFInput", "WFConditionalActionString"] : []
    _ = action.value(forKey: "serializedParameters")
    let actualVariables = (action.value(forKey: "containedVariables") as? [NSObject] ?? [])
        .reduce(into: Set<String>()) { result, variable in
            result.formUnion(variableKeys(in: variable.value(forKey: "serializedRepresentation") ?? [:]))
        }
    let missingVariables = variableKeys(in: parameters).subtracting(actualVariables)
    if !missingVariables.isEmpty {
        errors.append("Action \(index): variable references lost during native decoding: \(missingVariables.sorted())")
    }
    for key in parameters.keys where !keys.union(structural).union(legacyIf).union(legacyWorkflow).contains(key) {
        errors.append("Action \(index): unknown parameter \(key)")
    }
    _ = action.value(forKey: "parameters")
    for (key, value) in parameters where keys.contains(key) && value is String {
        guard let parameter = action.perform(NSSelectorFromString("parameterForKey:"), with: key)?.takeUnretainedValue() as? NSObject,
              parameter.responds(to: NSSelectorFromString("possibleStates")),
              let states = parameter.value(forKey: "possibleStates") as? [NSObject] else { continue }
        let values = states.compactMap { $0.value(forKey: "serializedRepresentation") as? String }
        if !values.isEmpty && !values.contains(value as! String) {
            errors.append("Action \(index): \(key) value \(value) is not one of \(values).")
        }
    }
    if identifier == "is.workflow.actions.conditional", parameters["WFControlFlowMode"] as? Int == 0 {
        let roundTrip = action.value(forKey: "serializedParameters") as? [String: Any] ?? [:]
        if let expected = parameters["WFInput"] as? NSDictionary,
           let actual = roundTrip["WFInput"] as? NSDictionary, expected == actual {
            // A malformed subject silently disappears in Apple's decoder.
        } else { errors.append("Action \(index): If condition lost its input during native decoding.") }
        if roundTrip["WFConditionalActionString"] as? String != parameters["WFConditionalActionString"] as? String {
            errors.append("Action \(index): If comparison changed during native decoding.")
        }
        if roundTrip["WFCondition"] as? Int != parameters["WFCondition"] as? Int {
            errors.append("Action \(index): If operator changed during native decoding.")
        }
    }
    if let uuid = parameters["UUID"] as? String, !uuids.insert(uuid).inserted {
        errors.append("Action \(index): duplicate UUID")
    }
    if let mode = parameters["WFControlFlowMode"] as? Int,
       let group = parameters["GroupingIdentifier"] as? String {
        if mode == 0 { groups.append(group) }
        else if groups.last != group { errors.append("Action \(index): unbalanced control flow") }
        else if mode == 2 { groups.removeLast() }
    }
    if identifier == "is.workflow.actions.documentpicker.save" {
        if parameters["WFAskWhereToSave"] as? Bool != false || parameters["WFSaveFileOverwrite"] as? Bool != false {
            errors.append("Save File must use the configured folder and must not overwrite existing captures.")
        }
        if parameters["WFFolder"] != nil { errors.append("Reusable Shortcut contains a machine-specific destination.") }
        if isLauncher {
            // The launcher contract checker verifies the complete helper-output range.
        } else if index == 0 {
            errors.append("Save File needs an explicitly named capture archive.")
        } else {
            let previous = actions[index - 1]
            let rename = previous["WFWorkflowActionParameters"] as? [String: Any] ?? [:]
            let name = (rename["WFName"] as? [String: Any])?["Value"] as? [String: Any]
            let input = (parameters["WFInput"] as? [String: Any])?["Value"] as? [String: Any]
            if previous["WFWorkflowActionIdentifier"] as? String != "is.workflow.actions.setitemname"
                || !(name?["string"] as? String ?? "").hasSuffix(".cuttingscapture.zip")
                || input?["OutputUUID"] as? String != rename["UUID"] as? String {
                errors.append("Save File must receive the explicitly renamed .cuttingscapture.zip file; Make Archive can ignore its name field.")
            }
        }
    }
    if identifier == "is.workflow.actions.setvalueforkey", parameters["WFDictionaryKey"] as? String == "attachments" {
        errors.append("Do not assign attachments through Set Dictionary Value: Shortcuts unwraps a singleton List into an object.")
    }
    if identifier == "is.workflow.actions.downloadurl", parameters["WFHTTPMethod"] as? String != "GET" {
        errors.append("Image capture must use GET.")
    }
    if (identifier.contains("download") && identifier != "is.workflow.actions.downloadurl") || identifier.contains("runjavascript") || identifier.contains("url.getcontents") {
        errors.append("Unexpected network or script action: \(identifier)")
    }
}
if !groups.isEmpty { errors.append("Unclosed control flow") }
let questions = workflow["WFWorkflowImportQuestions"] as? [[String: Any]] ?? []
if isLauncher {
    if questions.count != 1 || questions.first?["ParameterKey"] as? String != "WFFolder" || questions.first?["ActionIndex"] as? Int != 5 {
        errors.append("Fresh launcher setup must select the Save File destination.")
    }
} else {
    if !questions.isEmpty { errors.append("Helper must not ask for a destination.") }
    if actions.contains(where: { ["is.workflow.actions.documentpicker.save", "is.workflow.actions.notification"].contains($0["WFWorkflowActionIdentifier"] as? String ?? "") }) {
        errors.append("Helper must return results without saving or notifying.")
    }
    if !(workflow["WFWorkflowTypes"] as? [String] ?? []).isEmpty { errors.append("Helper must not appear in the Share Sheet.") }
}
if errors.isEmpty {
    print("Validated \(actions.count) actions, action parameters, control flow, and workflow boundaries.")
    print("Signing and on-device Share Sheet execution remain separate checks.")
} else {
    for error in errors { fputs(error + "\n", stderr) }
    exit(1)
}
