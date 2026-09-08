//
//  LauncherModel.swift
//  Paloma
//
//

import Observation

@MainActor
@Observable
final class LauncherModel {
    private(set) var connectorsHealth: HealthLevel = .inactive
    private(set) var pluginsHealth: HealthLevel = .inactive
    private(set) var connectors: [Connector] = []
    private(set) var preferredModel: String?
    private(set) var preferredEffort: String?

    @ObservationIgnored private var refreshTask: Task<Void, Never>?

    func refresh() {
        refreshTask?.cancel()
        refreshTask = Task {
            // Failures degrade to inactive indicators; core logs details.
            _ = await CoreClient.shared.withApp { app in
                async let connectorsLevel = app.connectorsHealthLevel()
                async let pluginsLevel = app.pluginsHealthLevel()
                async let availableList = app.availableConnectors()

                let health = await (try? connectorsLevel) ?? .inactive
                let plugins = await (try? pluginsLevel) ?? .inactive
                let available = await (try? availableList) ?? []
                let selection = Self.currentSelection(available)

                // A superseded refresh must not overwrite a newer one.
                guard !Task.isCancelled else { return }
                connectorsHealth = health
                pluginsHealth = plugins
                connectors = available
                preferredModel = selection?.name
                preferredEffort = selection?.effort
            }
        }
    }

    /// get the currect selected model and its effort
    private static func currentSelection(_ connectors: [Connector]) -> (name: String, effort: String)? {
        for connector in connectors {
            guard let connection = connector.connection else {
                continue
            }
            if !connection.preferred || connection.status.status != .running {
                continue
            }

            let preferredModel = connection.status.models.first { $0.id == connection.preferModel }
            guard let model = preferredModel else {
                continue
            }

            let efforts = model.supportedReasoningEfforts
            if !efforts.isEmpty, !efforts.contains(connection.preferEffort) {
                continue
            }

            return (name: model.name, effort: connection.preferEffort)
        }
        return nil
    }
}
