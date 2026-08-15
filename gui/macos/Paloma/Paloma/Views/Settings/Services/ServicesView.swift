//
//  ServicesView.swift
//  Paloma
//

import SwiftUI

struct ServicesView: View {
    let model: ServiceModel
    @State private var operationError: OperationError?
    @State private var connectingProvider: ProviderBackendId?

    var body: some View {
        Form {
            Section("Connected") {
                if model.connected.isEmpty {
                    Text("No services connected.")
                        .foregroundStyle(.secondary)
                }
                ForEach(model.connected, id: \.id) { connector in
                    ServiceRowView(connector: connector) {
                        OperationError.run("Failed to Disconnect Service", into: $operationError) {
                            await model.disconnect(connector.id)
                        }
                    }
                }
            }
            Section("Available") {
                if model.available.isEmpty {
                    Text("All supported services are connected.")
                        .foregroundStyle(.secondary)
                }
                ForEach(model.available, id: \.id) { connector in
                    HStack(spacing: 12) {
                        IconView(icon: connector.icon)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(connector.id.label)
                            Text(connector.description)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button("Connect") {
                            connectingProvider = connector.id
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .sheet(item: $connectingProvider) { providerBackendId in
            ServiceConnectionDialog(model: model, providerBackendId: providerBackendId) {
                connectingProvider = nil
            }
        }
        .operationErrorAlert($operationError)
    }
}
