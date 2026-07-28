//
//  PluginsView.swift
//  Scry
//

import SwiftUI

struct PluginsView: View {
    let model: PluginModel
    @State private var dialog: PluginDialogState?
    @State private var path = NavigationPath()
    @State private var operationError: OperationError?

    var body: some View {
        NavigationStack(path: $path) {
            Form {
                Section {
                    ForEach(model.extensions, id: \.name) { extensionInfo in
                        ExtensionRowView(extensionInfo: extensionInfo) {
                            path.append(extensionInfo)
                        } onEdit: {
                            if let config = extensionInfo.config {
                                dialog = PluginDialogState(.extension, editing: config)
                            }
                        } onDelete: {
                            OperationError.run("Failed to Remove Extension", into: $operationError) {
                                await model.removePlugin(.extension, extensionInfo.name)
                            }
                        }
                    }
                } header: {
                    HStack {
                        Text("Extensions")
                        Spacer()
                        Button {
                            dialog = PluginDialogState(.extension)
                        } label: {
                            Image(systemName: "plus")
                        }
                        .buttonStyle(.ghostIcon)
                        .help("Add an Extension")
                    }
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
                            path.append(server)
                        } onEdit: {
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
            .navigationDestination(for: ExtensionInfo.self) { extensionInfo in
                ExtensionCapabilitiesView(extensionInfo: extensionInfo) {
                    path.removeLast()
                }
            }
            .navigationDestination(for: McpPluginInfo.self) { server in
                McpToolsView(server: server) {
                    path.removeLast()
                }
            }
            .sheet(item: $dialog) { state in
                PluginDialog(model: model, onClose: { dialog = nil }, state: state)
            }
            .operationErrorAlert($operationError)
        }
    }
}
