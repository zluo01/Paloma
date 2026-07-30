//
//  PluginType+Label.swift
//  Paloma
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
