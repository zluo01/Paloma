//
//  PluginsView.swift
//  Paloma
//

import SwiftUI

struct PluginsView: View {
    let model: PluginModel
    @State private var dialog: PluginDialogState?
    @State private var path = NavigationPath()
    @State private var operationError: OperationError?

    private func runToggle(
        _ noun: String,
        disabled: Bool,
        _ operation: @escaping () async -> Result<Void, Error>
    ) {
        OperationError.run(
            disabled ? "Failed to Disable \(noun)" : "Failed to Enable \(noun)",
            into: $operationError,
            operation
        )
    }

    /// Nothing should be able to open a page for a plugin that is gone, so this
    /// reports rather than showing a stale one. If this shows, it indicates a bug.
    private func vanished(_ kind: String, _ name: String) -> some View {
        Color.clear.onAppear {
            operationError = OperationError(
                title: "\(kind) Unavailable",
                message: "\(name) is no longer installed."
            )
            path.removeLast()
        }
    }

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
                        } onToggle: { disabled in
                            runToggle("Extension", disabled: disabled) {
                                await model.togglePlugin(extensionInfo.name, disabled: disabled)
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
                            runToggle("MCP Server", disabled: disabled) {
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
            .navigationDestination(for: ExtensionInfo.self) { ext in
                // we need to get the object from model such that the switch becomes reactive
                // this is unlikely to cause error since extension is rendered from model
                if let extensionInfo = model.extensions.first(where: { $0.name == ext.name }) {
                    ExtensionCapabilitiesView(extensionInfo: extensionInfo) {
                        path.removeLast()
                    } onToggleCapability: { capability, facet, disabled in
                        runToggle("Capability", disabled: disabled) {
                            await model.toggleCapability(
                                .extension, extensionInfo.name, capability, facet: facet,
                                disabled: disabled
                            )
                        }
                    }
                } else {
                    vanished("Extension", ext.name)
                }
            }
            .navigationDestination(for: McpPluginInfo.self) { mcp in
                // same as above, if shows error, it means bug.
                if let server = model.mcps.first(where: { $0.config.name == mcp.config.name }) {
                    McpToolsView(server: server) {
                        path.removeLast()
                    } onToggleTool: { capability, disabled in
                        runToggle("Tool", disabled: disabled) {
                            await model.toggleCapability(
                                .mcp, server.config.name, capability, facet: .mcp, disabled: disabled
                            )
                        }
                    }
                } else {
                    vanished("MCP Server", mcp.config.name)
                }
            }
        }
        .sheet(item: $dialog) { state in
            PluginDialog(model: model, onClose: { dialog = nil }, state: state)
        }
        .operationErrorAlert($operationError)
    }
}
