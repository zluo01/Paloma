//
//  ServicesView.swift
//  Scry
//

import SwiftUI

struct ServicesView: View {
    let model: ServiceModel
    @State private var operationError: OperationError?
    @State private var connectingProvider: ProviderId?

    var body: some View {
        Form {
            Section("Connected") {
                if model.connected.isEmpty {
                    Text("No services connected.")
                        .foregroundStyle(.secondary)
                }
                ForEach(model.connected, id: \.id) { connector in
                    ServiceRowView(connector: connector) { chosenModel, effort in
                        OperationError.run("Failed to Set Preference", into: $operationError) {
                            await model.setPreference(connector.id, model: chosenModel, effort: effort)
                        }
                    } onDisconnect: {
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
                        Image(nsImage: connector.id.logo)
                            .resizable()
                            .frame(width: 26, height: 26)
                        Text(connector.id.label)
                        Spacer()
                        Button("Connect") {
                            connectingProvider = connector.id
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .sheet(item: $connectingProvider) { provider in
            ServiceConnectionDialog(model: model, provider: provider) {
                connectingProvider = nil
            }
        }
        .operationErrorAlert($operationError)
    }
}
