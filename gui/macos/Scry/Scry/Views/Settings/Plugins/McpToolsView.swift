//
//  McpToolsView.swift
//  Scry
//

import SwiftUI

struct McpToolsView: View {
    let server: McpPluginInfo
    let onBack: () -> Void
    let onToggleTool: (_ capability: String, _ disabled: Bool) -> Void

    var body: some View {
        Form {
            Section {
                if !server.description.isEmpty {
                    Text(server.description)
                        .foregroundStyle(.secondary)
                }
                if let error = server.error {
                    LabeledContent("Error", value: error)
                        .foregroundStyle(.red)
                }
            }
            Section {
                if server.tools.isEmpty {
                    Text(server.status == .starting ? "Connecting…" : "No tools.")
                        .foregroundStyle(.secondary)
                }
                ForEach(server.tools, id: \.id) { tool in
                    CapabilityRowView(
                        capability: tool,
                        facet: .mcp,
                        isPluginDisabled: server.config.disabled
                    ) { disabled in
                        onToggleTool(tool.id, disabled)
                    }
                }
            } header: {
                Text("Tools")
            }
        }
        .formStyle(.grouped)
        .navigationTitle(server.config.name)
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button(action: onBack) {
                    Image(systemName: "chevron.left")
                }
                .help("Back to Plugins")
            }
        }
    }
}
