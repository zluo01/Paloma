//
//  CoreClient.swift
//  Paloma
//

import Foundation
import Observation

@Observable
final class CoreClient {
    static let shared = CoreClient()

    private init() {}

    private(set) var app: PalomaApp?

    func bootstrap() async -> Result<Void, Error> {
        guard app == nil else { return .success(()) }
        do {
            let logsDir = try FileManager.default.url(
                for: .libraryDirectory, in: .userDomainMask,
                appropriateFor: nil, create: false
            ).appendingPathComponent("Logs", isDirectory: true)
            initLogging(logPath: logsDir.path())
            let dataDir = try FileManager.default.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: false
            )
            app = try await PalomaApp(appDataPath: dataDir.path)
            return .success(())
        } catch {
            return .failure(error)
        }
    }

    func withApp<T>(_ body: @MainActor (PalomaApp) async throws -> T) async -> Result<T, Error> {
        guard let app else { return .failure(PalomaError.Failure(message: "core not running.")) }
        do {
            return try await .success(body(app))
        } catch {
            return .failure(error)
        }
    }

    func setModelPreference(_ providerBackendId: ProviderBackendId, model: String, effort: String, setDefault: Bool) async -> Result<Void, Error> {
        await withApp { app in
            try await app.setModelPreference(providerBackendId: providerBackendId, model: model, effort: effort, setDefault: setDefault)
        }
    }

    /// The shared quiet failure policy: log the error, keep the previous value.
    @MainActor
    func load<T>(
        _ operation: @MainActor @escaping (PalomaApp) async throws -> T,
        or failure: String,
        category: String,
        into assign: @MainActor @escaping (T) -> Void
    ) {
        Task {
            switch await withApp(operation) {
            case let .success(value):
                assign(value)
            case let .failure(error):
                logError(target: category, message: "\(failure): \(String(describing: error))")
            }
        }
    }
}
