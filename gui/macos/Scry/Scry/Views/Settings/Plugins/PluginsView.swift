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
                if model.mcps.isEmpty {
                    Text("No MCP servers configured.")
                        .foregroundStyle(.secondary)
                }
                ForEach(model.mcps, id: \.config.name) { server in
                    PluginRowView(server: server) {
                        dialog = PluginDialogState(editing: server.config)
                    } onToggle: { disabled in
                        OperationError.run(
                            disabled ? "Failed to Disable MCP Server" : "Failed to Enable MCP Server",
                            into: $operationError
                        ) {
                            await model.togglePlugin(server.config.name, disabled: disabled)
                        }
                    } onDelete: {
                        OperationError.run("Failed to Remove MCP Server", into: $operationError) {
                            await model.removeMcp(server.config.name)
                        }
                    }
                }
            } header: {
                HStack {
                    Text("MCP servers")
                    Spacer()
                    Button {
                        dialog = PluginDialogState()
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
