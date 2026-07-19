//
//  PluginsView.swift
//  Scry
//

import SwiftUI

struct PluginsView: View {
    let model: PluginModel
    @State private var dialog: PluginDialogState?
    @State private var operationError: OperationError?

    var body: some View {
        Form {
            Section {
                Text("Native plugins are not supported yet.")
                    .foregroundStyle(.secondary)
            } header: {
                Text("Plugins")
            }
            Section {
                ForEach(model.providers, id: \.name) { provider in
                    ProviderRowView(provider: provider) {
                        if provider.config != nil {
                            dialog = PluginDialogState(.provider, editing: provider.config!)
                        }
                    } onDelete: {
                        OperationError.run("Failed to Remove Provider", into: $operationError) {
                            await model.removePlugin(.provider, provider.name)
                        }
                    }
                }
            } header: {
                HStack {
                    Text("Providers")
                    Spacer()
                    Button {
                        dialog = PluginDialogState(.provider)
                    } label: {
                        Image(systemName: "plus")
                    }
                    .buttonStyle(.ghostIcon)
                    .help("Add a Provider")
                }
            }
            Section {
                if model.mcps.isEmpty {
                    Text("No MCP servers configured.")
                        .foregroundStyle(.secondary)
                }
                ForEach(model.mcps, id: \.config.name) { server in
                    McpRowView(server: server) {
                        dialog = PluginDialogState(.mcp, editing: server.config)
                    } onToggle: { disabled in
                        OperationError.run(
                            disabled ? "Failed to Disable MCP Server" : "Failed to Enable MCP Server",
                            into: $operationError
                        ) {
                            await model.togglePlugin(server.config.name, disabled: disabled)
                        }
                    } onDelete: {
                        OperationError.run("Failed to Remove MCP Server", into: $operationError) {
                            await model.removePlugin(.mcp, server.config.name)
                        }
                    }
                }
            } header: {
                HStack {
                    Text("MCP Servers")
                    Spacer()
                    Button {
                        dialog = PluginDialogState(.mcp)
                    } label: {
                        Image(systemName: "plus")
                    }
                    .buttonStyle(.ghostIcon)
                    .help("Add an MCP server")
                }
            }
        }
        .formStyle(.grouped)
        .sheet(item: $dialog) { state in
            PluginDialog(model: model, onClose: { dialog = nil }, state: state)
        }
        .operationErrorAlert($operationError)
    }
}
