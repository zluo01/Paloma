//
//  ProviderId+Logo.swift
//  Scry
//

extension ProviderId {
    var logo: String {
        switch self {
        case .codex, .openAi: "OpenAILogo"
        case .claudeCode, .anthropic: "ClaudeLogo"
        }
    }
}
