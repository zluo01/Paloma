//
//  PluginModel.swift
//  Scry
//
//

import Observation

@MainActor
@Observable
final class PluginModel {
    private(set) var mcps: [McpServer] = []

    func refresh() {
        CoreClient.shared.load({ try await $0.listMcps() }, or: "failed to refresh MCP servers", category: "plugins") {
            self.mcps = $0
        }
    }

    func isPluginNameTaken(_ name: String) -> Bool {
        mcps.contains { $0.config.name == name }
    }

    func updatePlugin(_ config: Plugin) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.updatePlugin(pluginType: .mcp, plugin: config)
            refresh()
        }
    }

    func initMcpConnection(_ config: Plugin) async -> Result<McpOauthSession?, Error> {
        await CoreClient.shared.withApp { app in
            try await app.initMcpConnection(config: config)
        }
    }

    func finalizeMcpConnection(_ config: Plugin, session: McpOauthSession?) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.finalizeMcpConnection(config: config, session: session)
            refresh()
        }
    }

    func togglePlugin(_ name: String, disabled: Bool) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.togglePlugin(name: name, disabled: disabled)
            if let index = mcps.firstIndex(where: { $0.config.name == name }) {
                mcps[index].config.disabled = disabled
            }
        }
    }

    func removeMcp(_ name: String) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.removePlugin(pluginType: .mcp, name: name)
            mcps.removeAll { $0.config.name == name }
        }
    }
}
