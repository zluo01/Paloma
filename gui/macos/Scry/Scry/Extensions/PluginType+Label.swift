//
//  PluginType+Label.swift
//  Scry
//
//

extension PluginType {
    var label: String {
        switch self {
        case .extension: "Extension"
        case .provider: "Provider"
        case .mcp: "MCP Server"
        }
    }
}
