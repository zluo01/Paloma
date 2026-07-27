//
//  McpRowView.swift
//  Scry
//
//

import SwiftUI

struct McpRowView: View {
    let server: McpPluginInfo
    let onEdit: () -> Void
    let onToggle: (_ disabled: Bool) -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(server.config.name)
                Text(server.description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer()

            statusIcon

            if server.status != .starting {
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
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch server.status {
        case .running:
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
        case .starting:
            ProgressView().controlSize(.mini)
        case .unhealthy:
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
                .padding(5)
                .help(server.error ?? "unknown error")
        }
    }
}
