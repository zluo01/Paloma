//
//  ProviderId+Logo.swift
//  Scry
//

import AppKit

extension ProviderId {
    var logo: NSImage {
        let resource: String
        switch self {
        case .codex, .openAi:
            resource = "openai"
        case .claudeCode, .anthropic:
            resource = "claude"
        }

        guard let url = Bundle.main.url(forResource: resource, withExtension: "svg"),
              let image = NSImage(contentsOf: url)
        else { return NSImage() }
        return image
    }
}
