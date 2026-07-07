//
//  ProviderId+Label.swift
//  Scry
//
//  Lives outside Generated/ so bindgen runs do not overwrite it.
//

extension ProviderId {
    var label: String {
        switch self {
        case .codex: "Codex"
        case .claudeCode: "Claude Code"
        case .openAi: "OpenAI"
        case .anthropic: "Anthropic"
        }
    }
}
