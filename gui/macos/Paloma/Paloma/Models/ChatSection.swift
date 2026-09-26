//
//  ChatSection.swift
//  Paloma
//
//

import AppKit

enum ChatSection: Identifiable {
    case user(id: Int, text: String, images: [UInt32: NSImage])
    case assistant(id: Int, providerBackendId: ProviderBackendId, text: String)
    case reasoning(id: Int, text: String)
    case tool(ToolCallState)

    var id: Int {
        switch self {
        case let .user(id, _, _), let .assistant(id, _, _), let .reasoning(id, _):
            id
        case let .tool(state):
            state.id
        }
    }
}
