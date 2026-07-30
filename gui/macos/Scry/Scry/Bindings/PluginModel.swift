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
    private(set) var extensions: [ExtensionInfo] = []

    func refresh() {
        refreshExtensionsPlugins()
        refreshProviderPlugins()
        refreshMcpServers()
    }

    func refreshExtensionsPlugins() {
        CoreClient.shared.load({ try await $0.listExtensionPlugins() }, or: "failed to refresh extension plugins", category: "plugins") {
            self.extensions = $0
        }
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
                refreshExtensionsPlugins()
            case .provider:
                refreshProviderPlugins()
            case .mcp:
                refreshMcpServers()
            }
        }
    }

    func addExtensionPlugin(_ config: Plugin) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.addExtensionPlugin(config: config)
            refreshExtensionsPlugins()
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
            if let index = extensions.firstIndex(where: { $0.name == name }) {
                extensions[index].config?.disabled = disabled
            }
        }
    }

    func toggleCapability(
        _ pluginType: PluginType,
        _ name: String,
        _ capability: String,
        facet: CapabilityFacet,
        disabled: Bool
    ) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.toggleCapability(
                name: name, capability: capability, facet: facet, disabled: disabled
            )
            switch pluginType {
            case .extension:
                if let index = extensions.firstIndex(where: { $0.name == name }) {
                    Self.patch(&extensions[index].capabilities, capability, facet, disabled)
                }
            case .mcp:
                if let index = mcps.firstIndex(where: { $0.config.name == name }) {
                    Self.patch(&mcps[index].tools, capability, facet, disabled)
                }
            case .provider:
                break
            }
        }
    }

    private static func patch(
        _ capabilities: inout [CapabilityInfo],
        _ capability: String,
        _ facet: CapabilityFacet,
        _ disabled: Bool
    ) {
        guard let index = capabilities.firstIndex(where: { $0.id == capability }),
              let facetIndex = capabilities[index].facets.firstIndex(where: { $0.facet == facet })
        else {
            return
        }
        capabilities[index].facets[facetIndex].disabled = disabled
    }

    func removePlugin(_ pluginType: PluginType, _ name: String) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.removePlugin(pluginType: pluginType, name: name)
            switch pluginType {
            case .extension:
                extensions.removeAll { $0.name == name }
            case .provider:
                providers.removeAll { $0.name == name }
            case .mcp:
                mcps.removeAll { $0.config.name == name }
            }
        }
    }
}
