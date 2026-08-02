using Paloma.Models;
using CapabilityFacet = Paloma.Binding.V1.CapabilityFacet;
using Connector = Paloma.Binding.V1.Connector;
using ExtAction = Paloma.Extension.V1.Action;
using ExtensionCapabilityId = Paloma.Binding.V1.ExtensionCapabilityId;
using ExtensionInfo = Paloma.Binding.V1.ExtensionInfo;
using HealthLevel = Paloma.Binding.V1.HealthLevel;
using McpPluginInfo = Paloma.Binding.V1.McpPluginInfo;
using Permission = Paloma.Binding.V1.Permission;
using PermissionState = Paloma.Binding.V1.PermissionState;
using Plugin = Paloma.Binding.V1.Plugin;
using PluginType = Paloma.Binding.V1.PluginType;
using ProviderAuthMethod = Paloma.Provider.Runtime.V1.ProviderAuthMethod;
using ProviderBackendId = Paloma.Binding.V1.ProviderBackendId;
using ProviderInfo = Paloma.Binding.V1.ProviderInfo;
using QueryResponse = Paloma.Binding.V1.QueryResponse;
using RunActionResponse = Paloma.Extension.V1.RunActionResponse;
using SessionListItem = Paloma.Binding.V1.SessionListItem;
using UserDecision = Paloma.Binding.V1.UserDecision;

namespace Paloma.Client;

public interface IPalomaClient
{
    IAsyncEnumerable<QueryResponse> SearchAsync(
        string input,
        CancellationToken cancellationToken = default);

    Task<RunActionResponse?> RunSearchActionAsync(
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

    Task<(string? SessionId, string? AuthUrl)> InitMcpConnectionAsync(
        Plugin config,
        CancellationToken cancellationToken = default);

    Task FinalizeMcpConnectionAsync(
        Plugin config,
        string? oauthSessionId,
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