//
//  ModelPickerView.swift
//  Paloma
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
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                Image(systemName: "brain")
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
        let groups = modelProviders(providerBackendId, connection.status.models)
        return Menu {
            // display models directly on single model provider, else show the model providers menu first
            if groups.count == 1, let group = groups.first {
                ForEach(group.models, id: \.id) { item in
                    modelMenu(providerBackendId, connection, item)
                }
            } else {
                ForEach(groups, id: \.name) { group in
                    modelProviderMenu(providerBackendId, connection, group)
                }
            }
        } label: {
            menuLabel(providerBackendId.label, checked: connection.preferred)
        }
    }

    private func modelProviderMenu(_ providerBackendId: ProviderBackendId, _ connection: ConnectorConnection, _ group: (name: String, models: [Model])) -> some View {
        let hasCurrentModel = connection.preferred && group.models.contains { $0.id == connection.preferModel }
        return Menu {
            ForEach(group.models, id: \.id) { item in
                modelMenu(providerBackendId, connection, item)
            }
        } label: {
            menuLabel(group.name, checked: hasCurrentModel)
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

    /// group models by their model providers, sorted by provider name
    private func modelProviders(_ providerBackendId: ProviderBackendId, _ models: [Model]) -> [(name: String, models: [Model])] {
        var groups: [String: [Model]] = [:]
        for model in models {
            let provider = model.provider.isEmpty ? providerBackendId.label : model.provider
            groups[provider, default: []].append(model)
        }
        return groups
            .map { (name: $0.key, models: $0.value) }
            .sorted { $0.name < $1.name }
    }
}
