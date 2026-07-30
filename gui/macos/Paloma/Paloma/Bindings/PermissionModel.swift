//
//  PermissionModel.swift
//  Paloma
//
//

import Observation

@MainActor
@Observable
final class PermissionModel {
    private(set) var permissions: [Permission] = []

    func refresh() {
        CoreClient.shared.load({ try await $0.getPermissions() }, or: "failed to load permissions", category: "permissions") {
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
