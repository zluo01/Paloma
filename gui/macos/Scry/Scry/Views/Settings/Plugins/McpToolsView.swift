//
//  McpToolsView.swift
//  Scry
//

import SwiftUI

struct McpToolsView: View {
    let server: McpPluginInfo
    let onBack: () -> Void

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
                ForEach(server.tools, id: \.id, content: toolRow)
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

    private func toolRow(_ tool: CapabilityInfo) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(tool.id)
            if !tool.description.isEmpty {
                Text(tool.description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 2)
    }
}
