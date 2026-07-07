//
//  ToolCallState.swift
//  Scry
//
//

struct ToolCallState: Identifiable {
    let id: Int
    let name: String
    let arguments: String
    let description: String?
    let decisions: [UserDecision]
    var resolution: PermissionState?
}
