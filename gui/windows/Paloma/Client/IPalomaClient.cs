using Paloma.Models;
using Behavior = PalomaCore.Behavior;
using CapabilityFacet = PalomaCore.CapabilityFacet;
using Connector = PalomaCore.Connector;
using ExtAction = PalomaCore.Action;
using ExtensionCapabilityId = PalomaCore.ExtensionCapabilityId;
using ExtensionInfo = PalomaCore.ExtensionInfo;
using HealthLevel = PalomaCore.HealthLevel;
using McpOauthSession = PalomaCore.McpOauthSession;
using McpPluginInfo = PalomaCore.McpPluginInfo;
using Permission = PalomaCore.Permission;
using PermissionState = PalomaCore.PermissionState;
using Plugin = PalomaCore.Plugin;
using PluginType = PalomaCore.PluginType;
using ProviderAuthMethod = PalomaCore.ProviderAuthMethod;
using ProviderBackendId = PalomaCore.ProviderBackendId;
using ProviderInfo = PalomaCore.ProviderInfo;
using QueryResponse = PalomaCore.QueryResponse;
using SessionListItem = PalomaCore.SessionListItem;
using UserDecision = PalomaCore.UserDecision;

namespace Paloma.Client;

public interface IPalomaClient
{
    IAsyncEnumerable<QueryResponse> SearchAsync(
        string input,
        CancellationToken cancellationToken = default);

    Task<Behavior?> RunSearchActionAsync(
        ExtensionCapabilityId capabilityId,
        ExtAction action,
        CancellationToken cancellationToken = default);

    Task<ProviderBackendId?> PreferModelAsync(CancellationToken cancellationToken = default);

    IAsyncEnumerable<ChatStreamEvent> ChatAsync(
        string? sessionId,
        ProviderBackendId backend,
        string prompt,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<SessionListItem>> GetSessionsAsync(
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<string>> SearchSessionsAsync(
        string needle,
        CancellationToken cancellationToken = default);

    Task RemoveSessionAsync(string sessionId, CancellationToken cancellationToken = default);

    IAsyncEnumerable<ChatStreamEvent> RestoreSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default);

    Task CancelSessionAsync(string sessionId, CancellationToken cancellationToken = default);

    Task<PermissionState> DecideAsync(
        UserDecision decision,
        CancellationToken cancellationToken = default);

    Task<(HealthLevel Services, HealthLevel Plugins)> GetHealthAsync(
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<Connector>> GetConnectorsAsync(
        CancellationToken cancellationToken = default);

    Task<ConnectionPhase> InitConnectionAsync(
        ProviderBackendId id,
        CancellationToken cancellationToken = default);

    Task FinalizeConnectionAsync(
        ProviderBackendId id,
        ProviderAuthMethod method,
        string payload,
        CancellationToken cancellationToken = default);

    Task CancelConnectionAsync(ProviderBackendId id, CancellationToken cancellationToken = default);

    Task DisconnectAsync(ProviderBackendId id, CancellationToken cancellationToken = default);

    Task SetModelPreferenceAsync(
        ProviderBackendId id,
        string model,
        string effort,
        bool asDefault = false,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<ExtensionInfo>> GetExtensionPluginsAsync(
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<ProviderInfo>> GetProviderPluginsAsync(
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<McpPluginInfo>> GetMcpsAsync(CancellationToken cancellationToken = default);

    Task TogglePluginAsync(
        string name,
        bool disabled,
        CancellationToken cancellationToken = default);

    Task ToggleCapabilityAsync(
        string plugin,
        string capability,
        CapabilityFacet facet,
        bool disabled,
        CancellationToken cancellationToken = default);

    Task AddExtensionPluginAsync(
        Plugin config,
        CancellationToken cancellationToken = default);

    Task AddProviderPluginAsync(
        Plugin config,
        CancellationToken cancellationToken = default);

    Task<McpOauthSession?> InitMcpConnectionAsync(
        Plugin config,
        CancellationToken cancellationToken = default);

    Task FinalizeMcpConnectionAsync(
        Plugin config,
        McpOauthSession? session,
        CancellationToken cancellationToken = default);

    Task UpdatePluginAsync(
        PluginType kind,
        Plugin config,
        CancellationToken cancellationToken = default);

    Task RemovePluginAsync(
        PluginType kind,
        string name,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<Permission>> GetPermissionsAsync(
        CancellationToken cancellationToken = default);

    Task DeletePermissionAsync(string prefix, CancellationToken cancellationToken = default);
}