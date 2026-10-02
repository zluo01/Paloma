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
using UserPromptAttachment = PalomaCore.UserPromptAttachment;
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

    Task<Behavior?> RunSearchActionAsync(ExtensionCapabilityId capabilityId, ExtAction action);

    Task<ProviderBackendId?> PreferModelAsync(CancellationToken cancellationToken = default);

    IAsyncEnumerable<ChatStreamEvent> ChatAsync(
        string? sessionId,
        ProviderBackendId backend,
        string prompt,
        UserPromptAttachment[] attachments,
        CancellationToken cancellationToken = default);

    Task<IReadOnlyList<SessionListItem>> GetSessionsAsync();

    Task<IReadOnlyList<string>> SearchSessionsAsync(
        string needle,
        CancellationToken cancellationToken = default);

    Task RemoveSessionAsync(string sessionId);

    IAsyncEnumerable<ChatStreamEvent> RestoreSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default);

    Task CancelSessionAsync(string sessionId);

    Task<PermissionState> DecideAsync(UserDecision decision);

    Task<(HealthLevel Services, HealthLevel Plugins)> GetHealthAsync();

    Task<IReadOnlyList<Connector>> GetConnectorsAsync();

    Task<ConnectionPhase> InitConnectionAsync(ProviderBackendId id);

    Task FinalizeConnectionAsync(ProviderBackendId id, ProviderAuthMethod method, string payload);

    Task CancelConnectionAsync(ProviderBackendId id);

    Task DisconnectAsync(ProviderBackendId id);

    Task SetModelPreferenceAsync(ProviderBackendId id, string model, string effort, bool asDefault = false);

    Task<IReadOnlyList<ExtensionInfo>> GetExtensionPluginsAsync();

    Task<IReadOnlyList<ProviderInfo>> GetProviderPluginsAsync();

    Task<IReadOnlyList<McpPluginInfo>> GetMcpsAsync();

    Task TogglePluginAsync(string name, bool disabled);

    Task ToggleCapabilityAsync(string plugin, string capability, CapabilityFacet facet, bool disabled);

    Task AddExtensionPluginAsync(Plugin config);

    Task AddProviderPluginAsync(Plugin config);

    Task<McpOauthSession?> InitMcpConnectionAsync(Plugin config);

    Task FinalizeMcpConnectionAsync(
        Plugin config,
        McpOauthSession? session,
        CancellationToken cancellationToken = default);

    Task UpdatePluginAsync(PluginType kind, Plugin config);

    Task RemovePluginAsync(PluginType kind, string name);

    Task<IReadOnlyList<Permission>> GetPermissionsAsync();

    Task DeletePermissionAsync(string prefix);
}