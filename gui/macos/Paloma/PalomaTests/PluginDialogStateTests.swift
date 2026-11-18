//
//  PluginDialogStateTests.swift
//  PalomaTests
//

@testable import Paloma
import Testing

/// `canSubmit` is what enables the dialog's Add/Save button.
@MainActor
struct PluginDialogStateTests {
    // MARK: - Remote MCP server

    @Test(arguments: [
        RemoteAuth.none,
        .bearer(token: "secret"),
        .headers(headers: [:]),
        .oauth,
        .oauthClient(clientId: "paloma-desktop"),
    ])
    func givenCompleteRemoteServerWhenValidatingShouldAllowSubmit(auth: RemoteAuth) {
        #expect(canSubmit(remote { $0.auth = auth }))
    }

    @Test func givenRemoteServerWithEmptyUrlWhenValidatingShouldBlockSubmit() {
        expectBlocked(remote()) { $0.url = "  " }
    }

    @Test(arguments: ["mcp.example.com", "ftp://mcp.example.com", "https://", "not a url"])
    func givenRemoteServerWithInvalidUrlWhenValidatingShouldBlockSubmit(url: String) {
        expectBlocked(remote()) { $0.url = url }
    }

    @Test func givenBearerAuthWithEmptyTokenWhenValidatingShouldBlockSubmit() {
        expectBlocked(remote()) { $0.auth = .bearer(token: "  ") }
    }

    @Test func givenEmptyClientIdWhenValidatingShouldBlockSubmit() {
        expectBlocked(remote()) { $0.auth = .oauthClient(clientId: "  ") }
    }

    @Test func givenDuplicateHeaderNamesWhenValidatingShouldFlagThemAndBlockSubmit() {
        let duplicated = remote {
            $0.auth = .headers(headers: [:])
            $0.headers = [
                KeyValueRow(name: "X-API-Key", value: "one"),
                KeyValueRow(name: "Accept", value: "application/json"),
                KeyValueRow(name: " x-api-key ", value: "two"),
            ]
        }
        #expect(duplicated.duplicateHeaders == [duplicated.headers[0].id, duplicated.headers[2].id])
        #expect(!canSubmit(duplicated))

        var renamed = duplicated
        renamed.headers[2].name = "X-Tenant"
        #expect(renamed.duplicateHeaders.isEmpty)
        #expect(canSubmit(renamed))
    }

    @Test func givenRowsWithBlankNamesWhenBuildingTheAuthShouldDropThem() {
        let state = remote {
            $0.auth = .headers(headers: [:])
            $0.headers = [
                KeyValueRow(name: " X-API-Key ", value: " key-456 "),
                KeyValueRow(name: "", value: "orphan"),
                KeyValueRow(name: "  ", value: ""),
            ]
        }
        #expect(canSubmit(state))
        #expect(state.remoteAuth == .headers(headers: ["X-API-Key": "key-456"]))
    }

    @Test func givenRemoteServerWithLeftoverLocalFieldsWhenValidatingShouldAllowSubmit() {
        #expect(canSubmit(remote {
            $0.command = ""
            $0.argsJson = "not json"
        }))
    }

    // MARK: - Local MCP server

    @Test func givenCompleteLocalServerWhenValidatingShouldAllowSubmit() {
        #expect(canSubmit(local()))
    }

    @Test func givenLocalServerWithEmptyCommandWhenValidatingShouldBlockSubmit() {
        expectBlocked(local()) { $0.command = "  " }
    }

    @Test func givenLocalServerWithEmptyArgumentsWhenValidatingShouldBlockSubmit() {
        expectBlocked(local()) { $0.argsJson = "  " }
    }

    @Test(arguments: ["not json", "[]", "[1, 2]", "{\"flag\": \"value\"}"])
    func givenLocalServerWithInvalidArgumentsWhenValidatingShouldBlockSubmit(args: String) {
        expectBlocked(local()) { $0.argsJson = args }
    }

    @Test func givenLocalServerWithLeftoverRemoteFieldsWhenValidatingShouldAllowSubmit() {
        #expect(canSubmit(local {
            $0.url = "not a url"
            $0.auth = .bearer(token: "")
        }))
    }

    // MARK: - Any MCP server

    @Test func givenServerWithEmptyNameWhenValidatingShouldBlockSubmit() {
        expectBlocked(remote()) { $0.name = "  " }
        expectBlocked(local()) { $0.name = "  " }
    }

    @Test func givenNewServerWithTakenNameWhenValidatingShouldBlockSubmit() {
        let state = remote { $0.name = " linear " }
        #expect(canSubmit(state, taken: ["other"]))
        #expect(!canSubmit(state, taken: ["linear"]))
        #expect(state.nameError(isTaken: { $0 == "linear" }) != nil)
    }

    @Test func givenEditedServerWhenValidatingShouldAllowItsOwnName() {
        let state = PluginDialogState(.mcp, editing: Plugin(
            name: "linear", transport: .http, timeout: 300, disabled: false, env: [:],
            args: .remote(url: "https://mcp.example.com/mcp", auth: .oauth)
        ))
        #expect(canSubmit(state, taken: ["linear"]))
        #expect(state.nameError(isTaken: { $0 == "linear" }) == nil)
    }

    @Test func givenDuplicateEnvironmentNamesWhenValidatingShouldFlagThemAndBlockSubmit() {
        let rows = [
            KeyValueRow(name: "API_KEY", value: "one"),
            KeyValueRow(name: "HOME", value: "/tmp"),
            KeyValueRow(name: " API_KEY ", value: "two"),
        ]
        #expect(remote { $0.env = rows }.duplicateEnv == [rows[0].id, rows[2].id])
        expectBlocked(remote()) { $0.env = rows }
        expectBlocked(local()) { $0.env = rows }
    }

    @Test func givenEnvironmentNamesDifferingOnlyByCaseWhenValidatingShouldAllowSubmit() {
        let state = remote { $0.env = [KeyValueRow(name: "path", value: "one"), KeyValueRow(name: "PATH", value: "two")] }
        #expect(state.duplicateEnv.isEmpty)
        #expect(canSubmit(state))
    }

    @Test func givenEnvironmentRowsWithBlankNamesWhenBuildingTheEnvironmentShouldDropThem() {
        let state = remote {
            $0.env = [
                KeyValueRow(name: " API_KEY ", value: " secret "),
                KeyValueRow(name: "", value: "orphan"),
                KeyValueRow(name: "  ", value: ""),
            ]
        }
        #expect(canSubmit(state))
        #expect(state.env.dictionary == ["API_KEY": "secret"])
    }

    @Test func givenSavedServerWhenEditingShouldListItsEnvironmentAndHeadersSortedByName() {
        let state = PluginDialogState(.mcp, editing: Plugin(
            name: "linear", transport: .http, timeout: 300, disabled: false, env: ["PATH": "/bin", "API_KEY": "secret"],
            args: .remote(url: "https://mcp.example.com/mcp", auth: .headers(headers: ["X-Tenant": "acme", "Accept": "application/json"]))
        ))
        #expect(state.env.map(\.name) == ["API_KEY", "PATH"])
        #expect(state.env.dictionary == ["API_KEY": "secret", "PATH": "/bin"])
        #expect(state.headers.map(\.name) == ["Accept", "X-Tenant"])
        #expect(state.remoteAuth == .headers(headers: ["X-Tenant": "acme", "Accept": "application/json"]))
    }

    @Test(arguments: [PluginType.mcp, .extension, .provider])
    func givenNewDialogWhenOpenedShouldHaveNoEnvironmentRows(pluginType: PluginType) {
        #expect(PluginDialogState(pluginType).env.isEmpty)
    }

    @Test func givenSavedServerWithoutEnvironmentWhenEditingShouldHaveNoEnvironmentRows() {
        let state = PluginDialogState(.mcp, editing: Plugin(
            name: "files", transport: .local, timeout: 300, disabled: false, env: [:],
            args: .local(command: "npx", args: ["-y", "server-files"])
        ))
        #expect(state.env.isEmpty)
        #expect(canSubmit(state))
    }

    @Test func givenCustomHeadersWithNothingEnteredWhenOpenedShouldStartFromOneBlankRow() {
        #expect(PluginDialogState(.mcp).headers.count == 1)
        let saved = PluginDialogState(.mcp, editing: Plugin(
            name: "linear", transport: .http, timeout: 300, disabled: false, env: [:],
            args: .remote(url: "https://mcp.example.com/mcp", auth: .headers(headers: [:]))
        ))
        #expect(saved.headers.count == 1)
        #expect(saved.headers.dictionary.isEmpty)
    }

    @Test(arguments: [0, -1, 3601])
    func givenServerWithTimeoutOutOfRangeWhenValidatingShouldBlockSubmit(timeout: Int) {
        expectBlocked(remote()) { $0.timeout = timeout }
        expectBlocked(local()) { $0.timeout = timeout }
    }

    @Test(arguments: [1, 3600])
    func givenServerWithTimeoutAtTheLimitsWhenValidatingShouldAllowSubmit(timeout: Int) {
        #expect(canSubmit(remote { $0.timeout = timeout }))
    }

    // MARK: - Extension and provider

    @Test(arguments: [PluginType.extension, .provider])
    func givenPluginWithCommandOnlyWhenValidatingShouldAllowSubmit(pluginType: PluginType) {
        #expect(canSubmit(command(pluginType)))
    }

    @Test(arguments: [PluginType.extension, .provider])
    func givenPluginWithArgumentsAndEnvironmentWhenValidatingShouldAllowSubmit(pluginType: PluginType) {
        #expect(canSubmit(command(pluginType) {
            $0.argsJson = "[\"--log-level\", \"info\"]"
            $0.env = [KeyValueRow(name: "API_KEY", value: "secret")]
        }))
        #expect(canSubmit(command(pluginType) { $0.argsJson = "[]" }))
    }

    @Test(arguments: [PluginType.extension, .provider])
    func givenPluginWithEmptyCommandWhenValidatingShouldBlockSubmit(pluginType: PluginType) {
        expectBlocked(command(pluginType)) { $0.command = "  " }
    }

    @Test(arguments: [PluginType.extension, .provider])
    func givenPluginWithInvalidArgumentsWhenValidatingShouldBlockSubmit(pluginType: PluginType) {
        expectBlocked(command(pluginType)) { $0.argsJson = "not json" }
    }

    @Test(arguments: [PluginType.extension, .provider])
    func givenPluginWithDuplicateEnvironmentNamesWhenValidatingShouldBlockSubmit(pluginType: PluginType) {
        expectBlocked(command(pluginType)) {
            $0.env = [KeyValueRow(name: "API_KEY", value: "one"), KeyValueRow(name: "API_KEY", value: "two")]
        }
    }

    // MARK: - Forms the dialog lets through, with one change applied

    private func remote(_ change: (inout PluginDialogState) -> Void = { _ in }) -> PluginDialogState {
        var state = PluginDialogState(.mcp)
        state.name = "linear"
        state.isRemote = true
        state.url = "https://mcp.example.com/mcp"
        change(&state)
        return state
    }

    private func local(_ change: (inout PluginDialogState) -> Void = { _ in }) -> PluginDialogState {
        var state = PluginDialogState(.mcp)
        state.name = "files"
        state.command = "npx"
        state.argsJson = "[\"-y\", \"server-files\"]"
        change(&state)
        return state
    }

    private func command(_ pluginType: PluginType, _ change: (inout PluginDialogState) -> Void = { _ in }) -> PluginDialogState {
        var state = PluginDialogState(pluginType)
        state.command = "/usr/local/bin/plugin"
        change(&state)
        return state
    }

    private func canSubmit(_ state: PluginDialogState, taken: Set<String> = []) -> Bool {
        state.canSubmit(isNameTaken: taken.contains)
    }

    /// Checks `form` is submittable first, so `change` is the only thing blocking it.
    private func expectBlocked(
        _ form: PluginDialogState,
        sourceLocation: SourceLocation = #_sourceLocation,
        by change: (inout PluginDialogState) -> Void
    ) {
        var changed = form
        change(&changed)
        #expect(canSubmit(form), "the unchanged form should be submittable", sourceLocation: sourceLocation)
        #expect(!canSubmit(changed), sourceLocation: sourceLocation)
    }
}
