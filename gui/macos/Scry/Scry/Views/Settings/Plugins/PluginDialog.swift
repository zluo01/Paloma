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
            PluginDialogHeader(pluginType: state.pluginType, editing: state.originalName != nil)
            fields
            buttons
                .padding(.top)
        }
        .padding()
        .frame(maxWidth: 480)
        .sheet(item: $oauthSession) { session in
            VStack(alignment: .leading, spacing: 16) {
                McpAuthorizationHeader()
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
            switch state.pluginType {
            case .extension:
                EmptyView()
            case .provider:
                providerFormView
            case .mcp:
                mcpFormView
            }
        }
        .padding()
        .formStyle(.columns)
        .textFieldStyle(.roundedBorder)
    }

    @ViewBuilder
    private var mcpFormView: some View {
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

    @ViewBuilder
    private var providerFormView: some View {
        TextField("Command", text: $state.command, prompt: Text("/path/to/provider"))
            .fontDesign(.monospaced)
            .padding(.bottom, 6)
            .disabled(submitting)
        LabeledContent("Arguments") {
            captioned("Optional. A JSON array of strings.") {
                TextField(
                    "Arguments", text: $state.argsJson,
                    prompt: Text("[\"--log-level\", \"info\"]")
                )
                .labelsHidden()
                .fontDesign(.monospaced)
                .disabled(submitting)
                .validationFlag(argsError)
            }
        }
        LabeledContent("Environment") {
            captioned("Optional. A JSON object of strings.") {
                TextField("Environment", text: $state.envJson, prompt: Text("{\"API_KEY\": \"secret\"}"))
                    .labelsHidden()
                    .fontDesign(.monospaced)
                    .disabled(submitting)
                    .validationFlag(envError)
            }
        }
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
        guard let args = try? JSONDecoder().decode([String].self, from: Data(text.utf8)) else { return nil }
        // A provider binary may take no arguments; MCP servers always need them.
        if state.pluginType == .mcp, args.isEmpty {
            return nil
        }
        return args
    }

    private var parsedEnv: [String: String]? {
        let text = state.envJson.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return [:] }
        return try? JSONDecoder().decode([String: String].self, from: Data(text.utf8))
    }

    private var argsError: String? {
        guard !state.argsJson.trimmingCharacters(in: .whitespaces).isEmpty else { return nil }
        guard parsedArgs == nil else { return nil }
        return state.pluginType == .mcp
            ? "Must be a non-empty JSON array like [\"--flag\", \"value\"]."
            : "Must be a JSON array like [\"--log-level\", \"info\"]."
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
        if state.pluginType == .provider {
            return !state.command.trimmingCharacters(in: .whitespaces).isEmpty
                && argsError == nil
                && envError == nil
        }
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

    private func submit() {
        guard let plugin = constructPlugin() else { return }

        operationError = nil
        submitting = true
        if state.originalName != nil {
            OperationError.run("Failed to Update \(state.pluginType.label)", into: $operationError) {
                await model.updatePlugin(state.pluginType, plugin)
            } onSuccess: {
                onClose()
            } onFailure: {
                submitting = false
            }
        } else {
            if state.pluginType == .mcp {
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
            } else {
                OperationError.run("Failed to Add Provider", into: $operationError) {
                    await model.addProviderPlugin(plugin)
                } onSuccess: {
                    onClose()
                } onFailure: {
                    submitting = false
                }
            }
        }
    }

    /// canSubmit gates the button; the guards only protect the invariant.
    private func constructPlugin() -> Plugin? {
        guard let env = parsedEnv else { return nil }
        switch state.pluginType {
        case .extension:
            // Unreachable: the extension form is empty, so canSubmit never enables.
            fatalError("extension plugins cannot be configured")
        case .provider:
            return Plugin(
                name: "",
                transport: .local,
                timeout: UInt32(state.timeout),
                disabled: false,
                env: env,
                args: .local(
                    command: state.command.trimmingCharacters(in: .whitespaces),
                    args: parsedArgs ?? []
                )
            )
        case .mcp:
            let args: PluginArgs
            if state.isRemote {
                args = .remote(url: state.url.trimmingCharacters(in: .whitespaces), requiresAuth: state.requiresAuth)
            } else {
                guard let parsed = parsedArgs else { return nil }
                args = .local(
                    command: state.command.trimmingCharacters(in: .whitespaces),
                    args: parsed
                )
            }
            return Plugin(
                name: state.name.trimmingCharacters(in: .whitespaces),
                transport: state.isRemote ? .http : .local,
                timeout: UInt32(state.timeout),
                disabled: state.disabled,
                env: env,
                args: args
            )
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

private struct McpAuthorizationHeader: View {
    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Image("MCPLogo")
                .resizable()
                .scaledToFit()
                .padding(4)
                .frame(width: 40, height: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text("Waiting for Authorization")
                    .font(.headline)
                Text("Approve the connection in your browser to finish adding this server.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
    }
}

struct PluginDialogState: Identifiable {
    var id: String {
        originalName ?? "new-plugin"
    }

    /// Set when editing an existing plugin.
    let originalName: String?
    let pluginType: PluginType
    var name = ""
    var isRemote = false
    var command = ""
    var argsJson = ""
    var url = ""
    var requiresAuth = false
    var timeout = 300
    var envJson = "{}"
    var disabled = false

    init(_ pluginType: PluginType) {
        originalName = nil
        self.pluginType = pluginType
    }

    init(_ pluginType: PluginType, editing plugin: Plugin) {
        originalName = plugin.name
        self.pluginType = pluginType
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
