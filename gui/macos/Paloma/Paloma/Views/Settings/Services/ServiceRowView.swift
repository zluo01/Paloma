//
//  ServiceRowView.swift
//  Paloma
//
//

import SwiftUI

struct ServiceRowView: View {
    let connector: Connector
    let onSetPreference: (_ model: String, _ effort: String) -> Void
    let onDisconnect: () -> Void

    private var connection: ConnectorConnection? {
        connector.connection
    }

    private var models: [Model] {
        connection?.status.models ?? []
    }

    private var current: Model? {
        models.first { $0.id == connection?.preferModel } ?? models.first
    }

    var body: some View {
        HStack(spacing: 12) {
            IconView(icon: connector.icon)
            VStack(alignment: .leading, spacing: 2) {
                Text(connector.id.label)
                statusText
            }
            Spacer()
            if connection?.status.status == .running, !models.isEmpty {
                pickers
            }
            Button(action: onDisconnect) {
                Image(systemName: "trash")
            }
            .buttonStyle(.ghostIcon)
            .help("Disconnect")
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private var statusText: some View {
        switch connection?.status.status {
        case .running, nil:
            Text(connector.description).font(.caption).foregroundStyle(.secondary)
        case .unhealthy:
            Text(connection?.status.error ?? "Connection error")
                .font(.caption)
                .foregroundStyle(.red)
                .lineLimit(2)
        case .starting:
            Text("Connecting…").font(.caption).foregroundStyle(.secondary)
        }
    }

    private var pickers: some View {
        HStack(spacing: 8) {
            Picker(
                "Model",
                selection: .init(
                    get: { connection?.preferModel ?? "" },
                    set: { id in
                        let chosen = models.first { $0.id == id }
                        onSetPreference(id, chosen?.defaultReasoningEffort ?? "")
                    }
                )
            ) {
                ForEach(models, id: \.id) { entry in
                    Text(entry.name).tag(entry.id)
                }
            }
            .help("Model")
            Picker(
                "Effort",
                selection: .init(
                    get: { connection?.preferEffort ?? "" },
                    set: { effort in
                        onSetPreference(connection?.preferModel ?? "", effort)
                    }
                )
            ) {
                ForEach(current?.supportedReasoningEfforts ?? [], id: \.self) { effort in
                    Text(effort).tag(effort)
                }
            }
            .help("Reasoning effort")
            .disabled((current?.supportedReasoningEfforts ?? []).isEmpty)
        }
        .pickerStyle(.menu)
        .controlSize(.small)
        .labelsHidden()
        .fixedSize()
    }
}
