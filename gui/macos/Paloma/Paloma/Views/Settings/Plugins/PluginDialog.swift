//
//  PluginDialog.swift
//  Paloma
//
//

import SwiftUI

struct PluginDialog: View {
    let model: PluginModel
    let onClose: () -> Void
    @State var state: PluginDialogState
    @State private var operationError: OperationError?
    @State private var oauthSession: McpOauthSession?
    @State private var finalizeTask: Task<Result<Void, Error>, Never>?
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
                        finalizeTask?.cancel()
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
            case .extension, .provider:
                commandFormView
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
                .validationFlag(state.nameError(isTaken: model.isPluginNameTaken))
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
                    .validationFlag(state.urlError)
            }
            .padding(.bottom, 6)
            Picker("Authentication", selection: $state.auth.kind) {
                ForEach(RemoteAuth.kinds, id: \.self) { kind in
                    Text(kind.label).tag(kind)
                }
            }
            .padding(.bottom, 3)
            .disabled(submitting)
            authDetailView
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
                    .validationFlag(state.argsError)
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
                    .validationFlag(state.timeoutError)
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
                    .validationFlag(state.envError)
            }
        }
    }

    @ViewBuilder
    private var authDetailView: some View {
        switch state.auth {
        case .none, .oauth:
            EmptyView()
        case let .bearer(token):
            LabeledContent("Token") {
                SecureField("Token", text: Binding(get: { token }, set: { state.auth = .bearer(token: $0) }))
                    .labelsHidden()
                    .disabled(submitting)
            }
            .padding(.top, 3)
            .padding(.bottom, 3)
        case .headers:
            LabeledContent("Headers") {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach($state.headers) { $header in
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            TextField("Name", text: $header.name, prompt: Text("Name"))
                                .labelsHidden()
                                .validationFlag(state.duplicateHeaders.contains(header.id) ? "Duplicate name." : nil)
                            TextField("Value", text: $header.value, prompt: Text("Value"))
                                .labelsHidden()
                            Button {
                                state.headers.removeAll { $0.id == header.id }
                            } label: {
                                Image(systemName: "trash")
                            }
                            .buttonStyle(.ghostIcon)
                            .help("Remove")
                        }
                    }
                    Button("Add Header", systemImage: "plus") {
                        state.headers.append(HeaderField())
                    }
                }
                .disabled(submitting)
            }
            .padding(.top, 3)
            .padding(.bottom, 3)
        case let .oauthClient(clientId):
            LabeledContent("Client ID") {
                TextField("Client ID", text: Binding(get: { clientId }, set: { state.auth = .oauthClient(clientId: $0) }))
                    .labelsHidden()
                    .disabled(submitting)
            }
            .padding(.top, 3)
            .padding(.bottom, 3)
        }
    }

    @ViewBuilder
    private var commandFormView: some View {
        TextField("Command", text: $state.command, prompt: Text("/path/to/\(state.pluginType.label.lowercased())"))
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
                .validationFlag(state.argsError)
            }
        }
        LabeledContent("Environment") {
            captioned("Optional. A JSON object of strings.") {
                TextField("Environment", text: $state.envJson, prompt: Text("{\"API_KEY\": \"secret\"}"))
                    .labelsHidden()
                    .fontDesign(.monospaced)
                    .disabled(submitting)
                    .validationFlag(state.envError)
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
                .disabled(submitting || !state.canSubmit(isNameTaken: model.isPluginNameTaken))
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
        } else if state.pluginType == .mcp {
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
            OperationError.run("Failed to Add \(state.pluginType.label)", into: $operationError) {
                if state.pluginType == .extension {
                    await model.addExtensionPlugin(plugin)
                } else {
                    await model.addProviderPlugin(plugin)
                }
            } onSuccess: {
                onClose()
            } onFailure: {
                submitting = false
            }
        }
    }

    /// canSubmit gates the button; the guards only protect the invariant.
    private func constructPlugin() -> Plugin? {
        guard let env = state.parsedEnv else { return nil }
        switch state.pluginType {
        case .extension, .provider:
            return Plugin(
                name: "",
                transport: .local,
                timeout: UInt32(state.timeout),
                disabled: false,
                env: env,
                args: .local(
                    command: state.command.trimmingCharacters(in: .whitespaces),
                    args: state.parsedArgs ?? []
                )
            )
        case .mcp:
            let args: PluginArgs
            if state.isRemote {
                guard let auth = state.remoteAuth else { return nil }
                args = .remote(url: state.url.trimmingCharacters(in: .whitespaces), auth: auth)
            } else {
                guard let parsed = state.parsedArgs else { return nil }
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
        let task = model.finalizeMcpConnection(plugin, session: session)
        finalizeTask = task
        OperationError.run("Failed to Add MCP Server", into: $operationError) {
            await task.value
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
    var auth = RemoteAuth.none
    var headers = [HeaderField()]
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
        case let .remote(url, auth):
            isRemote = true
            self.url = url
            self.auth = auth
            if case let .headers(headers) = auth {
                self.headers = headers.sorted { $0.key < $1.key }.map { HeaderField(name: $0.key, value: $0.value) }
            }
        }
    }

    // MARK: - Live per-field validation

    func nameError(isTaken: (String) -> Bool) -> String? {
        let name = name.trimmingCharacters(in: .whitespaces)
        return originalName == nil && isTaken(name) ? "A plugin with this name already exists." : nil
    }

    var parsedArgs: [String]? {
        let text = argsJson.trimmingCharacters(in: .whitespaces)
        guard let args = try? JSONDecoder().decode([String].self, from: Data(text.utf8)) else { return nil }
        // A provider binary may take no arguments; MCP servers always need them.
        if pluginType == .mcp, args.isEmpty {
            return nil
        }
        return args
    }

    var parsedEnv: [String: String]? {
        let text = envJson.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return [:] }
        return try? JSONDecoder().decode([String: String].self, from: Data(text.utf8))
    }

    var argsError: String? {
        guard !argsJson.trimmingCharacters(in: .whitespaces).isEmpty, parsedArgs == nil else { return nil }
        return pluginType == .mcp
            ? "Must be a non-empty JSON array like [\"--flag\", \"value\"]."
            : "Must be a JSON array like [\"--log-level\", \"info\"]."
    }

    var urlError: String? {
        let text = url.trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return nil }
        guard let url = URL(string: text), let scheme = url.scheme,
              ["http", "https"].contains(scheme), url.host() != nil
        else {
            return "Must be a valid http(s) URL."
        }
        return nil
    }

    var envError: String? {
        parsedEnv == nil ? "Must be a JSON object like {\"KEY\": \"value\"}." : nil
    }

    var timeoutError: String? {
        (1 ... 3600).contains(timeout) ? nil : "Must be between 1 and 3600."
    }

    func canSubmit(isNameTaken: (String) -> Bool) -> Bool {
        if pluginType != .mcp {
            return !command.trimmingCharacters(in: .whitespaces).isEmpty
                && argsError == nil
                && envError == nil
        }
        let requiredFilled = if isRemote {
            !url.trimmingCharacters(in: .whitespaces).isEmpty && remoteAuth != nil
        } else {
            !command.trimmingCharacters(in: .whitespaces).isEmpty
                && !argsJson.trimmingCharacters(in: .whitespaces).isEmpty
        }
        return requiredFilled
            && !name.trimmingCharacters(in: .whitespaces).isEmpty
            && nameError(isTaken: isNameTaken) == nil
            && (isRemote ? urlError : argsError) == nil
            && envError == nil
            && timeoutError == nil
    }

    /// Rows whose name another row also uses; header names are case-insensitive.
    var duplicateHeaders: Set<UUID> {
        let rows = Dictionary(grouping: headers) { $0.name.trimmingCharacters(in: .whitespaces).lowercased() }
        return Set(rows.filter { !$0.key.isEmpty && $0.value.count > 1 }.values.joined().map(\.id))
    }

    /// The auth to submit, or nil while it still needs input.
    var remoteAuth: RemoteAuth? {
        switch auth {
        case .none, .oauth:
            return auth
        case let .bearer(token):
            let token = token.trimmingCharacters(in: .whitespaces)
            return token.isEmpty ? nil : .bearer(token: token)
        case .headers:
            guard duplicateHeaders.isEmpty else { return nil }
            let headers = headers
                .map { ($0.name.trimmingCharacters(in: .whitespaces), $0.value.trimmingCharacters(in: .whitespaces)) }
                .filter { !$0.0.isEmpty }
            return .headers(headers: Dictionary(headers, uniquingKeysWith: { _, last in last }))
        case let .oauthClient(clientId):
            let clientId = clientId.trimmingCharacters(in: .whitespaces)
            return clientId.isEmpty ? nil : .oauthClient(clientId: clientId)
        }
    }

    private static func json(_ value: any Encodable) -> String {
        guard let data = try? JSONEncoder().encode(value) else { return "" }
        return String(data: data, encoding: .utf8) ?? ""
    }
}

struct HeaderField: Identifiable {
    let id = UUID()
    var name = ""
    var value = ""
}
