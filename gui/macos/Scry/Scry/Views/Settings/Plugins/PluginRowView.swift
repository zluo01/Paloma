//
//  PluginRowView.swift
//  Scry
//
//

import SwiftUI

struct PluginRowView: View {
    let server: McpServer
    let onEdit: () -> Void
    let onToggle: (_ disabled: Bool) -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            statusIcon
            VStack(alignment: .leading, spacing: 2) {
                Text(server.config.name)
                if !server.description.isEmpty {
                    Text(server.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
            Spacer()

            if server.status == .running {
                Toggle(
                    "",
                    isOn: .init(
                        get: { !server.config.disabled },
                        set: { enabled in
                            onToggle(!enabled)
                        }
                    )
                )
                .toggleStyle(.switch)
                .controlSize(.small)
                .labelsHidden()
            }

            Button(action: onEdit) {
                Image(systemName: "pencil")
            }
            .buttonStyle(.ghostIcon)
            .help("Edit")

            Button(action: onDelete) {
                Image(systemName: "trash")
            }
            .buttonStyle(.ghostIcon)
            .help("Remove")
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch server.status {
        case .running:
            Image(systemName: "circle.fill").font(.system(size: 8)).foregroundStyle(.green)
        case .starting:
            ProgressView().controlSize(.mini)
        case .unhealthy:
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 10))
                .foregroundStyle(.red)
                .help(server.error ?? "unknown error")
        }
    }
}
