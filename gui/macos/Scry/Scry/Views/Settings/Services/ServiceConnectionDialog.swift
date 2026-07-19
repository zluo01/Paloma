//
//  ServiceConnectionDialog.swift
//  Scry
//
//

import SwiftUI

struct ServiceConnectionDialog: View {
    let model: ServiceModel
    let providerBackendId: ProviderBackendId
    let onClose: () -> Void
    @State private var key = ""
    @State private var phase: ServiceConnectionPhase = .loading

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Connect \(providerBackendId.label)")
                .font(.headline)
            content
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) {
                    model.cancelConnection(providerBackendId)
                    onClose()
                }
                .keyboardShortcut(.cancelAction)
                if isInputStage {
                    Button("Connect", action: submit)
                        .keyboardShortcut(.defaultAction)
                        .disabled(key.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
        }
        .padding(20)
        .frame(width: 420)
        .task {
            phase = await model.initConnection(providerBackendId)
            // challenge should trigger finalization automatically
            if case .challenge = phase {
                guard let (method, payload) = phase.finalizePayload() else { return }
                phase = await model.finalizeConnection(method, providerBackendId, payload)
            }
        }
    }

    /// Stages that wait on typed input before finalize can run.
    private var isInputStage: Bool {
        switch phase {
        case .manual, .oauth: true
        default: false
        }
    }

    private func submit() {
        guard let (method, payload) = phase.finalizePayload(input: key.trimmingCharacters(in: .whitespaces)) else {
            return
        }
        // Back to the loading page while finalize runs
        phase = .loading
        Task {
            phase = await model.finalizeConnection(method, providerBackendId, payload)
        }
    }

    @ViewBuilder
    private var content: some View {
        switch phase {
        case .loading:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Preparing connection…").foregroundStyle(.secondary)
            }
        case let .challenge(verificationUrl, userCode, _):
            VStack(alignment: .leading, spacing: 10) {
                Text("Enter this code in your browser")
                    .foregroundStyle(.secondary)
                Text(userCode)
                    .font(.system(.title2, design: .monospaced).weight(.semibold))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity)
                if let url = URL(string: verificationUrl) {
                    Link("Open in browser", destination: url)
                        .font(.caption)
                }
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text("Waiting for approval…").foregroundStyle(.secondary)
                }
                .padding(.top, 2)
            }
        case let .manual(instructionsUrl):
            VStack(alignment: .leading, spacing: 8) {
                SecureField("API key", text: $key)
                    .textFieldStyle(.roundedBorder)
                if let instructionsUrl, let url = URL(string: instructionsUrl) {
                    Link("Get an API key", destination: url)
                        .font(.caption)
                }
            }
        case let .oauth(authorizationUrl):
            VStack(alignment: .leading, spacing: 8) {
                Text("Finish the sign-in in your browser, then paste the returned code or callback URL.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                TextField("Authorization code", text: $key)
                    .textFieldStyle(.roundedBorder)
                if let url = URL(string: authorizationUrl) {
                    Link("Open the authorization page", destination: url)
                        .font(.caption)
                }
            }
        case .success:
            HStack(spacing: 8) {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                Text("Connected")
            }
            .frame(maxWidth: .infinity)
            .task {
                // Leave the confirmation visible briefly, then close.
                try? await Task.sleep(for: .milliseconds(800))
                onClose()
            }
        case let .failed(message):
            Label {
                Text(message)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            } icon: {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.red)
            }
            .font(.callout)
        }
    }
}
