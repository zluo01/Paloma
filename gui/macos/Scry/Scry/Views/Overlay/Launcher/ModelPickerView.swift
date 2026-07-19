//
//  ModelPickerView.swift
//  Scry
//
//

import SwiftUI

/// Provider → model → effort, checkmarking the current preference.
struct ModelPickerView: View {
    let connectors: [Connector]
    let preferredModel: String?
    let preferredEffort: String?
    let onSelect: (ProviderBackendId, String, String) -> Void

    var body: some View {
        Menu {
            ForEach(connectors, id: \.id) { connector in
                if let connection = connector.connection {
                    providerMenu(connector.id, connection)
                }
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "cpu")
                    .font(.system(size: 10))
                Text(title)
                    .font(.caption)
                    .lineLimit(1)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 8, weight: .semibold))
            }
            .foregroundStyle(preferredModel == nil ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.secondary))
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(.quaternary.opacity(0.5), in: Capsule())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Preferred model")
    }

    private var title: String {
        guard let preferredModel else {
            return hasConnections ? "Select model" : "No model"
        }
        guard let preferredEffort, !preferredEffort.isEmpty else { return preferredModel }
        return "\(preferredModel) · \(preferredEffort)"
    }

    private var hasConnections: Bool {
        connectors.contains { $0.connection != nil }
    }

    private func providerMenu(_ providerBackendId: ProviderBackendId, _ connection: ConnectorConnection) -> some View {
        Menu {
            ForEach(connection.status.models, id: \.id) { item in
                modelMenu(providerBackendId, connection, item)
            }
        } label: {
            menuLabel(providerBackendId.label, checked: connection.preferred)
        }
    }

    @ViewBuilder
    private func modelMenu(_ providerBackendId: ProviderBackendId, _ connection: ConnectorConnection, _ item: Model) -> some View {
        let isCurrentModel = connection.preferred && connection.preferModel == item.id
        if item.supportedReasoningEfforts.isEmpty {
            Button {
                onSelect(providerBackendId, item.id, item.defaultReasoningEffort)
            } label: {
                menuLabel(item.name, checked: isCurrentModel)
            }
        } else {
            Menu {
                ForEach(item.supportedReasoningEfforts, id: \.self) { effort in
                    Button {
                        onSelect(providerBackendId, item.id, effort)
                    } label: {
                        menuLabel(effort, checked: isCurrentModel && connection.preferEffort == effort)
                    }
                }
            } label: {
                menuLabel(item.name, checked: isCurrentModel)
            }
        }
    }

    @ViewBuilder
    private func menuLabel(_ title: String, checked: Bool) -> some View {
        if checked {
            Label(title, systemImage: "checkmark")
        } else {
            Text(title)
        }
    }
}
