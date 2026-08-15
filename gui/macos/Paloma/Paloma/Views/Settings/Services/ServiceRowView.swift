//
//  ServiceRowView.swift
//  Paloma
//
//

import SwiftUI

struct ServiceRowView: View {
    let connector: Connector
    let onDisconnect: () -> Void

    private var connection: ConnectorConnection? {
        connector.connection
    }

    var body: some View {
        HStack(spacing: 12) {
            IconView(icon: connector.icon)
            VStack(alignment: .leading, spacing: 2) {
                Text(connector.id.label)
                statusText
            }
            Spacer()
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
}
