//
//  PluginModel.swift
//  Scry
//
//

import Observation

@MainActor
@Observable
final class PluginModel {
    private(set) var providers: [ProviderInfo] = []
    private(set) var mcps: [McpPluginInfo] = []

    func refresh() {
        refreshProviderPlugins()
        refreshMcpServers()
    }

    func refreshProviderPlugins() {
        CoreClient.shared.load({ try await $0.listProviderPlugins() }, or: "failed to refresh provider plugins", category: "plugins") {
            self.providers = $0
        }
    }

    func refreshMcpServers() {
        CoreClient.shared.load({ try await $0.listMcps() }, or: "failed to refresh MCP servers", category: "plugins") {
            self.mcps = $0
        }
    }

    func isPluginNameTaken(_ name: String) -> Bool {
        mcps.contains { $0.config.name == name } || providers.contains { $0.name == name }
    }

    func updatePlugin(_ pluginType: PluginType, _ config: Plugin) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.updatePlugin(pluginType: pluginType, plugin: config)
            switch pluginType {
            case .extension:
                break
            case .provider:
                refreshProviderPlugins()
            case .mcp:
                refreshMcpServers()
            }
        }
    }

    func addProviderPlugin(_ config: Plugin) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.addProviderPlugin(config: config)
            refreshProviderPlugins()
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
            refreshMcpServers()
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

    func removePlugin(_ pluginType: PluginType, _ name: String) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.removePlugin(pluginType: pluginType, name: name)
            switch pluginType {
            case .extension:
                break
            case .provider:
                providers.removeAll { $0.name == name }
            case .mcp:
                mcps.removeAll { $0.config.name == name }
            }
        }
    }
}
