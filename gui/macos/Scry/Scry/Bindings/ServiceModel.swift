//
//  ServiceModel.swift
//  Scry
//
//

import Observation

enum ServiceConnectionPhase {
    /// init_connection in flight.
    case loading
    /// otp challenge
    case challenge(verificationUrl: String, userCode: String, transactionPayloadJson: String)
    /// api key
    case manual(instructionsUrl: String?)
    /// oauth pkce
    case oauth(authorizationUrl: String)
    case success
    case failed(String)

    func connection(input: String = "") -> Connection? {
        switch self {
        case let .challenge(verificationUrl, userCode, transactionPayloadJson):
            .deviceCode(
                verificationUri: verificationUrl,
                userCode: userCode,
                transactionPayloadJson: transactionPayloadJson
            )
        case let .manual(instructionsUrl):
            .manualInput(apiKey: input, instructionsUrl: instructionsUrl)
        case .oauth:
            .browserRedirect(authorizationUrl: input)
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

    func initConnection(_ provider: ProviderId) async -> ServiceConnectionPhase {
        let result = await CoreClient.shared.withApp { app in
            try await app.initConnection(providerId: provider)
        }
        switch result {
        case let .success(connection):
            switch connection {
            case let .manualInput(_, instructionsUrl):
                return .manual(instructionsUrl: instructionsUrl)
            case let .deviceCode(uri, code, transactionPayloadJson):
                openUrl(uri)
                return .challenge(verificationUrl: uri, userCode: code, transactionPayloadJson: transactionPayloadJson)
            case let .browserRedirect(url):
                openUrl(url)
                return .oauth(authorizationUrl: url)
            }
        case let .failure(error):
            return .failed(error.displayMessage)
        }
    }

    func finalizeConnection(_ provider: ProviderId, payload: Connection) async -> ServiceConnectionPhase {
        let result = await CoreClient.shared.withApp { app in
            try await app.finalizeConnection(providerId: provider, payload: payload)
        }
        switch result {
        case .success:
            refresh()
            return .success
        case let .failure(error):
            return .failed(error.displayMessage)
        }
    }

    func cancelConnection() {
        CoreClient.shared.app?.cancelConnection()
    }

    func disconnect(_ provider: ProviderId) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.disconnectConnector(providerId: provider)
            refresh()
        }
    }

    func setPreference(_ provider: ProviderId, model: String, effort: String) async -> Result<Void, Error> {
        let result = await CoreClient.shared.setModelPreference(provider, model: model, effort: effort)
        if case .success = result, let index = services.firstIndex(where: { $0.id == provider }) {
            services[index].connection?.preferModel = model
            services[index].connection?.preferEffort = effort
        }
        return result
    }
}
