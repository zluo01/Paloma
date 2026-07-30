//
//  ChatSection.swift
//  Paloma
//
//

enum ChatSection: Identifiable {
    case user(id: Int, text: String)
    case assistant(id: Int, providerBackendId: ProviderBackendId, text: String)
    case reasoning(id: Int, text: String)
    case tool(ToolCallState)

    var id: Int {
        switch self {
        case let .user(id, _), let .assistant(id, _, _), let .reasoning(id, _):
            id
        case let .tool(state):
            state.id
        }
    }
}
