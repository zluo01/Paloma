//
//  LauncherModel.swift
//  Scry
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
                let connection = available
                    .first { $0.connection?.preferred == true }?
                    .connection

                // A superseded refresh must not overwrite a newer one.
                guard !Task.isCancelled else { return }
                connectorsHealth = health
                pluginsHealth = plugins
                connectors = available
                preferredModel = connection?.preferModel
                preferredEffort = connection?.preferEffort
            }
        }
    }
}
