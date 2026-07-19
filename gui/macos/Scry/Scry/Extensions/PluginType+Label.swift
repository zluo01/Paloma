//
//  PluginType+Label.swift
//  Scry
//
//

extension PluginType {
    var label: String {
        switch self {
        case .native: "Plugin"
        case .provider: "Provider"
        case .mcp: "MCP Server"
        }
    }
}
