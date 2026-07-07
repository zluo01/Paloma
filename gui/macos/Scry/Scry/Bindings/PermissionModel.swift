//
//  PermissionModel.swift
//  Scry
//
//

import Observation
import os

@MainActor
@Observable
final class PermissionModel {
    private(set) var permissions: [Permission] = []

    @ObservationIgnored private let logger = Logger(
        subsystem: "scry.settings", category: "permissions"
    )

    func refresh() {
        CoreClient.shared.load({ try await $0.getPermissions() }, or: "failed to load permissions", logger: logger) {
            self.permissions = $0
        }
    }

    func delete(_ prefix: String) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.deletePermission(prefix: prefix)
            permissions.removeAll { $0.prefix == prefix }
        }
    }
}
