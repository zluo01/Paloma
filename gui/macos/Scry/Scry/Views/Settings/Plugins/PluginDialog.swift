//
//  PluginDialog.swift
//  Scry
//
//

import SwiftUI

struct PluginDialog: View {
    let model: PluginModel
    let onClose: () -> Void
    @State var state: PluginDialogState
    @State private var operationError: OperationError?
    @State private var oauthSession: McpOauthSession?
    @State private var submitting = false

    var body: some View {
        VStack(spacing: 0) {
            PluginDialogHeader(
                title: state.originalName == nil ? "Add MCP Server" : "Edit MCP Server",
                subtitle: "Connect a Model Context Protocol server."
            )
            fields
            buttons
                .padding(.top)
        }
        .padding()
        .frame(maxWidth: 480)
        .sheet(item: $oauthSession) { session in
            VStack(alignment: .leading, spacing: 16) {
                PluginDialogHeader(
                    title: "Waiting for Authorization",
                    subtitle: "Approve the connection in your browser to finish adding this server."
                )
                if let url = URL(string: session.authUrl()) {
                    Link(destination: url) {
                        Label("Open the authorization page", systemImage: "arrow.up.forward.square")
                    }
                    .font(.callout)
                    .padding(.leading, 52)
                }
                HStack(spacing: 8) {
                    ProgressView()
                        .controlSize(.small)
                    Text("Listening for the response…")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Cancel", role: .cancel) {
                        session.cancel()
                        submitting = false
                    }
                    .keyboardShortcut(.cancelAction)
                }
                .padding(.top, 4)
            }
            .padding(20)
            .frame(width: 460)
        }
        .operationErrorAlert($operationError)
    }

    private var fields: some View {
        Form {
            LabeledContent("Name") {
                TextField("Name", text: $state.name)
                    .labelsHidden()
                    .disabled(state.originalName != nil || submitting)
                    .validationFlag(nameError)
            }
            .padding(.bottom, 6)
            Picker("Type", selection: $state.isRemote) {
                Text("Local command").tag(false)
                Text("Remote server").tag(true)
            }
            .padding(.bottom, 6)
            .pickerStyle(.segmented)
            .disabled(submitting)

            if state.isRemote {
                LabeledContent("URL") {
                    TextField("URL", text: $state.url, prompt: Text("https://example.com/mcp"))
                        .labelsHidden()
                        .disabled(submitting)
                        .validationFlag(urlError)
                }
                .padding(.bottom, 6)
                LabeledContent("") {
                    Toggle("Requires authentication", isOn: $state.requiresAuth)
                        .disabled(submitting)
                }.padding(.bottom, 3)
            } else {
                TextField("Command", text: $state.command, prompt: Text("npx"))
                    .fontDesign(.monospaced)
                    .padding(.bottom, 6)
                    .disabled(submitting)
                LabeledContent("Arguments") {
                    captioned("A JSON array of strings.") {
                        TextField(
                            "Arguments", text: $state.argsJson,
                            prompt: Text("[\"--flag\", \"value\"]")
                        )
                        .labelsHidden()
                        .fontDesign(.monospaced)
                        .disabled(submitting)
                        .validationFlag(argsError)
                    }
                }
            }
            Divider()
                .padding(.bottom, 6)
                .padding(.top, 3)
            LabeledContent("Timeout") {
                HStack(spacing: 6) {
                    TextField("Timeout", value: $state.timeout, format: .number)
                        .labelsHidden()
                        .frame(width: 70)
                        .disabled(submitting)
                        .validationFlag(timeoutError)
                    Text("seconds")
                        .foregroundStyle(.secondary)
                }
            }
            LabeledContent("Environment") {
                captioned("A JSON object of strings.") {
                    TextField("Environment", text: $state.envJson, prompt: Text("{\"KEY\": \"value\"}"))
                        .labelsHidden()
                        .fontDesign(.monospaced)
                        .disabled(submitting)
                        .validationFlag(envError)
                }
            }
        }
        .padding()
        .formStyle(.columns)
        .textFieldStyle(.roundedBorder)
    }

    private var buttons: some View {
        HStack {
            Button("Cancel", role: .cancel, action: onClose)
                .disabled(submitting)
                .keyboardShortcut(.cancelAction)
            Button(state.originalName == nil ? "Add" : "Save", action: submit)
                .keyboardShortcut(.defaultAction)
                .disabled(submitting || !canSubmit)
                .buttonStyle(.borderedProminent)
        }
    }

    private func captioned(_ caption: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            content()
            Text(caption)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Live per-field validation

    private var nameError: String? {
        let name = state.name.trimmingCharacters(in: .whitespaces)
        return state.originalName == nil && model.isPluginNameTaken(name) ? "A plugin with this name already exists." : nil
    }

    private var parsedArgs: [String]? {
        let text = state.argsJson.trimmingCharacters(in: .whitespaces)
        guard let args = try? JSONDecoder().decode([String].self, from: Data(text.utf8)),
              !args.isEmpty
        else { return nil }
        return args
    }

    private var parsedEnv: [String: String]? {
        let text = state.envJson.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return [:] }
        return try? JSONDecoder().decode([String: String].self, from: Data(text.utf8))
    }

    private var argsError: String? {
        guard !state.argsJson.trimmingCharacters(in: .whitespaces).isEmpty else { return nil }
        return parsedArgs == nil ? "Must be a non-empty JSON array like [\"--flag\", \"value\"]." : nil
    }

    private var urlError: String? {
        let text = state.url.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return nil }
        guard let url = URL(string: text), let scheme = url.scheme,
              ["http", "https"].contains(scheme), url.host() != nil
        else {
            return "Must be a valid http(s) URL."
        }
        return nil
    }

    private var envError: String? {
        parsedEnv == nil ? "Must be a JSON object like {\"KEY\": \"value\"}." : nil
    }

    private var timeoutError: String? {
        (1 ... 3600).contains(state.timeout) ? nil : "Must be between 1 and 3600."
    }

    private var canSubmit: Bool {
        let requiredFilled = if state.isRemote {
            !state.url.trimmingCharacters(in: .whitespaces).isEmpty
        } else {
            !state.command.trimmingCharacters(in: .whitespaces).isEmpty
                && !state.argsJson.trimmingCharacters(in: .whitespaces).isEmpty
        }
        return requiredFilled
            && !state.name.trimmingCharacters(in: .whitespaces).isEmpty
            && nameError == nil
            && (state.isRemote ? urlError : argsError) == nil
            && envError == nil
            && timeoutError == nil
    }

    /// canSubmit gates the button; the guards only protect the invariant.
    private func submit() {
        guard let env = parsedEnv else { return }

        let args: PluginArgs
        if state.isRemote {
            args = .remote(url: state.url.trimmingCharacters(in: .whitespaces), requiresAuth: state.requiresAuth)
        } else {
            guard let parsed = parsedArgs else { return }
            args = .local(
                command: state.command.trimmingCharacters(in: .whitespaces),
                args: parsed
            )
        }

        let plugin = Plugin(
            name: state.name.trimmingCharacters(in: .whitespaces),
            transport: state.isRemote ? .http : .local,
            timeout: UInt32(state.timeout),
            disabled: state.disabled,
            env: env,
            args: args
        )

        operationError = nil
        submitting = true
        if state.originalName != nil {
            OperationError.run("Failed to Update MCP Server", into: $operationError) {
                await model.updatePlugin(plugin)
            } onSuccess: {
                onClose()
            } onFailure: {
                submitting = false
            }
        } else {
            OperationError.run("Failed to Add MCP Server", into: $operationError) {
                await model.initMcpConnection(plugin)
            } onSuccess: { session in
                oauthSession = session
                if let session {
                    openUrl(session.authUrl())
                }
                finalizeConnection(plugin, session: session)
            } onFailure: {
                submitting = false
            }
        }
    }

    private func finalizeConnection(_ plugin: Plugin, session: McpOauthSession?) {
        OperationError.run("Failed to Add MCP Server", into: $operationError) {
            await model.finalizeMcpConnection(plugin, session: session)
        } onSuccess: {
            oauthSession = nil
            onClose()
        } onFailure: {
            oauthSession = nil
            if submitting {
                submitting = false
            } else {
                // Cancelled from the OAuth sheet; the failure is expected.
                operationError = nil
            }
        }
    }
}

struct PluginDialogState: Identifiable {
    var id: String {
        originalName ?? "new-plugin"
    }

    /// Set when editing an existing plugin.
    let originalName: String?
    var name = ""
    var isRemote = false
    var command = ""
    var argsJson = ""
    var url = ""
    var requiresAuth = false
    var timeout = 300
    var envJson = "{}"
    var disabled = false

    init() {
        originalName = nil
    }

    init(editing plugin: Plugin) {
        originalName = plugin.name
        name = plugin.name
        timeout = Int(plugin.timeout)
        envJson = Self.json(plugin.env)
        disabled = plugin.disabled
        switch plugin.args {
        case let .local(command, args):
            isRemote = false
            self.command = command
            argsJson = Self.json(args)
        case let .remote(url, requiresAuth):
            isRemote = true
            self.url = url
            self.requiresAuth = requiresAuth
        }
    }

    private static func json(_ value: any Encodable) -> String {
        guard let data = try? JSONEncoder().encode(value) else { return "" }
        return String(data: data, encoding: .utf8) ?? ""
    }
}
