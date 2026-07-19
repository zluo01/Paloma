//
//  PluginDialogHeader.swift
//  Scry
//

import SwiftUI

struct PluginDialogHeader: View {
    let pluginType: PluginType
    let editing: Bool

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            dialogIcon
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.headline)
                Text(subtitle)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
    }

    @ViewBuilder
    private var dialogIcon: some View {
        switch pluginType {
        case .mcp:
            Image("MCPLogo")
                .resizable()
                .scaledToFit()
                .padding(4)
                .frame(width: 40, height: 40)
        case .provider:
            Image(systemName: "brain")
                .resizable()
                .scaledToFit()
                .padding(4)
                .frame(width: 40, height: 40)
        case .native:
            Image(systemName: "cpu")
                .resizable()
                .scaledToFit()
                .padding(4)
                .frame(width: 40, height: 40)
        }
    }

    private var title: String {
        (editing ? "Edit " : "Add ") + pluginType.label
    }

    private var subtitle: String {
        switch pluginType {
        case .native: "Configure a native plugin."
        case .provider: "Connect a provider plugin."
        case .mcp: "Connect a Model Context Protocol server."
        }
    }
}
