//
//  ServiceModel.swift
//  Paloma
//
//

import Observation

enum ServiceConnectionPhase {
    /// init_connection in flight.
    case loading
    /// otp challenge
    case challenge(verificationUrl: String, userCode: String, transactionPayload: String)
    /// api key
    case manual(instructionsUrl: String?)
    /// oauth pkce
    case oauth(authorizationUrl: String)
    case success
    case failed(String)

    func finalizePayload(input: String = "") -> (ProviderAuthMethod, String)? {
        switch self {
        case let .challenge(_, _, transactionPayload):
            (.deviceCode, transactionPayload)
        case .manual:
            (.apiKey, input)
        case .oauth:
            (.browserOauth, input)
        case .loading, .success, .failed:
            nil
        }
    }
}

@MainActor
@Observable
final class ServiceModel {
    private(set) var services: [Connector] = []

    func refresh() {
        CoreClient.shared.load({ try await $0.availableConnectors() }, or: "failed to refresh connectors", category: "services") {
            self.services = $0
        }
    }

    var connected: [Connector] {
        services.filter { $0.connection != nil }
    }

    var available: [Connector] {
        services.filter { $0.connection == nil }
    }

    func initConnection(_ providerBackendId: ProviderBackendId) async -> ServiceConnectionPhase {
        let result = await CoreClient.shared.withApp { app in
            try await app.initConnection(providerBackendId: providerBackendId)
        }
        switch result {
        case let .success(connection):
            switch connection {
            case let .manualInput(instructionsUrl):
                return .manual(instructionsUrl: instructionsUrl)
            case let .deviceCode(url, code, transactionPayload):
                openUrl(url)
                return .challenge(verificationUrl: url, userCode: code, transactionPayload: transactionPayload)
            case let .browserRedirect(url):
                openUrl(url)
                return .oauth(authorizationUrl: url)
            }
        case let .failure(error):
            return .failed(error.displayMessage)
        }
    }

    func finalizeConnection(_ providerAuthMethod: ProviderAuthMethod, _ providerBackendId: ProviderBackendId, _ payload: String) async -> ServiceConnectionPhase {
        let result = await CoreClient.shared.withApp { app in
            try await app.finalizeConnection(providerAuthMethod: providerAuthMethod, providerBackendId: providerBackendId, payload: payload)
        }
        switch result {
        case .success:
            refresh()
            return .success
        case let .failure(error):
            return .failed(error.displayMessage)
        }
    }

    func cancelConnection(_ providerBackendId: ProviderBackendId) {
        CoreClient.shared.load({ try await $0.cancelConnection(providerBackendId: providerBackendId) }, or: "failed to cancel connection", category: "services") { _ in }
    }

    func disconnect(_ providerBackendId: ProviderBackendId) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.disconnectConnector(providerBackendId: providerBackendId)
            refresh()
        }
    }

    func setPreference(_ providerBackendId: ProviderBackendId, model: String, effort: String) async -> Result<Void, Error> {
        let result = await CoreClient.shared.setModelPreference(providerBackendId, model: model, effort: effort, setDefault: false)
        if case .success = result, let index = services.firstIndex(where: { $0.id == providerBackendId }) {
            services[index].connection?.preferModel = model
            services[index].connection?.preferEffort = effort
        }
        return result
    }
}
