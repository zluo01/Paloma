using System.Runtime.CompilerServices;
using Paloma.Client;
using Paloma.Models;
using CapabilityFacet = PalomaCore.CapabilityFacet;
using Connector = PalomaCore.Connector;
using ExtAction = PalomaCore.Action;
using ExtensionCapabilityId = PalomaCore.ExtensionCapabilityId;
using ExtensionInfo = PalomaCore.ExtensionInfo;
using HealthLevel = PalomaCore.HealthLevel;
using McpOauthSession = PalomaCore.McpOauthSession;
using McpPluginInfo = PalomaCore.McpPluginInfo;
using ProviderInfo = PalomaCore.ProviderInfo;
using Permission = PalomaCore.Permission;
using PermissionState = PalomaCore.PermissionState;
using Plugin = PalomaCore.Plugin;
using PluginType = PalomaCore.PluginType;
using ProviderAuthMethod = PalomaCore.ProviderAuthMethod;
using ProviderBackendId = PalomaCore.ProviderBackendId;
using QueryResponse = PalomaCore.QueryResponse;
using Behavior = PalomaCore.Behavior;
using SessionListItem = PalomaCore.SessionListItem;
using UserDecision = PalomaCore.UserDecision;

namespace Paloma.Tests;

/// <summary>Configurable mock: set the delegate for what a test exercises;
/// everything else answers with an empty success. Cancellation surfaces as
/// OperationCanceledException, the way the real client reports it.</summary>
internal sealed class MockPalomaClient : IPalomaClient
{
    private static OperationCanceledException Cancelled() => new("call was cancelled");

    private static void ThrowIfCancelled(CancellationToken token)
    {
        if (token.IsCancellationRequested)
        {
            throw Cancelled();
        }
    }

    private static async IAsyncEnumerable<T> Observing<T>(
        IAsyncEnumerable<T> source,
        [EnumeratorCancellation] CancellationToken token)
    {
        await using var enumerator = source.GetAsyncEnumerator(token);
        while (true)
        {
            // Honor the call token even for sources that ignore theirs.
            ThrowIfCancelled(token);
            bool moved;
            try
            {
                moved = await enumerator.MoveNextAsync();
            }
            catch (OperationCanceledException)
            {
                throw Cancelled();
            }

            if (!moved)
            {
                yield break;
            }

            yield return enumerator.Current;
        }
    }

    public List<(string? SessionId, string Prompt)> ChatCalls { get; } = [];

    public List<(string Name, bool Disabled)> PluginToggles { get; } = [];

    public List<(string Plugin, string Capability, CapabilityFacet Facet, bool Disabled)> CapabilityToggles { get; } =
        [];

    public List<(ProviderBackendId Id, string Model, string Effort)> ModelPreferences { get; } = [];

    public List<string> CancelledSessions { get; } = [];

    public Func<string?, string, IAsyncEnumerable<ChatStreamEvent>>? OnChat { get; set; }

    public Func<string, IAsyncEnumerable<ChatStreamEvent>>? OnRestore { get; set; }

    public Func<string, CancellationToken, IAsyncEnumerable<QueryResponse>>? OnSearch { get; set; }

    public Func<UserDecision, PermissionState>? OnDecide { get; set; }

    public Func<UserDecision, Task<PermissionState>>? OnDecideAsync { get; set; }

    public Func<ExtensionCapabilityId, ExtAction, Behavior>? OnRunAction { get; set; }

    public Func<ExtensionCapabilityId, ExtAction, Task<Behavior>>? OnRunActionAsync { get; set; }

    public Action<string>? OnRemoveSession { get; set; }

    public Action<string, bool>? OnTogglePlugin { get; set; }

    public Func<string, IReadOnlyList<string>>? OnSearchSessions { get; set; }

    public Func<string, CancellationToken, Task<IReadOnlyList<string>>>? OnSearchSessionsAsync { get; set; }

    public Func<ProviderBackendId, ConnectionPhase>? OnInitConnection { get; set; }

    public Action<ProviderBackendId>? OnDisconnect { get; set; }

    public Action<PluginType, string>? OnRemovePlugin { get; set; }

    public Action<Plugin>? OnAddExtensionPlugin { get; set; }

    public List<(PluginType Kind, Plugin Config)> UpdatedPlugins { get; } = [];

    public List<Plugin> FinalizedMcps { get; } = [];

    public Action<ProviderBackendId, string, string>? OnSetModelPreference { get; set; }

    public List<(ProviderBackendId Id, ProviderAuthMethod Method, string Payload)> FinalizeConnections { get; } = [];

    public IReadOnlyList<Permission> Permissions { get; set; } = [];

    public Action<string>? OnDeletePermission { get; set; }

    public List<string> DeletedPermissions { get; } = [];

    public List<string> SearchCalls { get; } = [];

    public List<string> SessionSearchCalls { get; } = [];

    public IReadOnlyList<SessionListItem> Sessions { get; set; } = [];

    public IReadOnlyList<Connector> Connectors { get; set; } = [];

    public Func<IReadOnlyList<Connector>>? OnGetConnectors { get; set; }

    public IReadOnlyList<ExtensionInfo> ExtensionPlugins { get; set; } = [];

    public IReadOnlyList<ProviderInfo> ProviderPlugins { get; set; } = [];

    public IReadOnlyList<McpPluginInfo> McpPlugins { get; set; } = [];

    public ProviderBackendId? PreferredBackend { get; set; } =
        new("provider", "backend");

    // The real client stops reading after a terminal event; the mock must
    // not deliver stream shapes production can never produce.
    private static async IAsyncEnumerable<ChatStreamEvent> Terminated(
        IAsyncEnumerable<ChatStreamEvent> source,
        [EnumeratorCancellation] CancellationToken token = default)
    {
        await foreach (var e in source.WithCancellation(token))
        {
            yield return e;
            if (PalomaClient.IsTerminal(e))
            {
                yield break;
            }
        }
    }

    public static async IAsyncEnumerable<T> Stream<T>(params T[] items)
    {
        foreach (var item in items)
        {
            yield return item;
        }

        await Task.CompletedTask;
    }

    public IAsyncEnumerable<QueryResponse> SearchAsync(
        string input,
        CancellationToken cancellationToken = default)
    {
        SearchCalls.Add(input);
        return Observing(
            OnSearch?.Invoke(input, cancellationToken) ?? Stream<QueryResponse>(),
            cancellationToken);
    }

    public async Task<Behavior?> RunSearchActionAsync(
        ExtensionCapabilityId capabilityId,
        ExtAction action,
        CancellationToken cancellationToken = default)
    {
        if (OnRunActionAsync is { } hook)
        {
            return await hook(capabilityId, action);
        }

        return OnRunAction?.Invoke(capabilityId, action) ?? new Behavior.Stay();
    }

    public Task<ProviderBackendId?> PreferModelAsync(CancellationToken cancellationToken = default)
    {
        ThrowIfCancelled(cancellationToken);
        return Task.FromResult<ProviderBackendId?>(PreferredBackend);
    }

    public IAsyncEnumerable<ChatStreamEvent> ChatAsync(
        string? sessionId,
        ProviderBackendId backend,
        string prompt,
        CancellationToken cancellationToken = default)
    {
        ChatCalls.Add((sessionId, prompt));
        return Observing(
            Terminated(
                OnChat?.Invoke(sessionId, prompt)
                ?? Stream<ChatStreamEvent>(new ChatStreamEvent.Done()),
                cancellationToken),
            cancellationToken);
    }

    public Task<IReadOnlyList<SessionListItem>> GetSessionsAsync(
        CancellationToken cancellationToken = default) =>
        Task.FromResult(Sessions);

    public async Task<IReadOnlyList<string>> SearchSessionsAsync(
        string needle,
        CancellationToken cancellationToken = default)
    {
        ThrowIfCancelled(cancellationToken);
        SessionSearchCalls.Add(needle);
        try
        {
            if (OnSearchSessionsAsync is { } hook)
            {
                return await hook(needle, cancellationToken);
            }
        }
        catch (OperationCanceledException)
        {
            throw Cancelled();
        }

        return OnSearchSessions?.Invoke(needle) ?? [];
    }

    public Task RemoveSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
    {
        OnRemoveSession?.Invoke(sessionId);
        return Task.CompletedTask;
    }

    public IAsyncEnumerable<ChatStreamEvent> RestoreSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
    {
        return Observing(
            Terminated(OnRestore?.Invoke(sessionId) ?? Stream<ChatStreamEvent>(), cancellationToken),
            cancellationToken);
    }

    public Task CancelSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
    {
        CancelledSessions.Add(sessionId);
        return Task.CompletedTask;
    }

    public Task<PermissionState> DecideAsync(
        UserDecision decision,
        CancellationToken cancellationToken = default) =>
        OnDecideAsync?.Invoke(decision)
        ?? Task.FromResult(OnDecide?.Invoke(decision) ?? PermissionState.Allow);

    public Task<(HealthLevel Services, HealthLevel Plugins)> GetHealthAsync(
        CancellationToken cancellationToken = default) =>
        Task.FromResult((HealthLevel.Healthy, HealthLevel.Healthy));

    public Task<IReadOnlyList<Connector>> GetConnectorsAsync(
        CancellationToken cancellationToken = default) =>
        Task.FromResult(OnGetConnectors is { } hook ? hook() : Connectors);

    public Task<ConnectionPhase> InitConnectionAsync(
        ProviderBackendId id,
        CancellationToken cancellationToken = default) =>
        // The real init can never return Success; its unreachable phases
        // must stay unreachable in tests too.
        Task.FromResult(OnInitConnection?.Invoke(id)
                        ?? (ConnectionPhase)new ConnectionPhase.Failed("no init hook configured"));

    public Task FinalizeConnectionAsync(
        ProviderBackendId id,
        ProviderAuthMethod method,
        string payload,
        CancellationToken cancellationToken = default)
    {
        FinalizeConnections.Add((id, method, payload));
        return Task.CompletedTask;
    }

    public Task CancelConnectionAsync(
        ProviderBackendId id,
        CancellationToken cancellationToken = default) =>
        Task.CompletedTask;

    public Task DisconnectAsync(ProviderBackendId id, CancellationToken cancellationToken = default)
    {
        OnDisconnect?.Invoke(id);
        return Task.CompletedTask;
    }

    public Task SetModelPreferenceAsync(
        ProviderBackendId id,
        string model,
        string effort,
        bool asDefault = false,
        CancellationToken cancellationToken = default)
    {
        OnSetModelPreference?.Invoke(id, model, effort);
        ModelPreferences.Add((id, model, effort));
        return Task.CompletedTask;
    }

    public Task<IReadOnlyList<ExtensionInfo>> GetExtensionPluginsAsync(
        CancellationToken cancellationToken = default) =>
        Task.FromResult(ExtensionPlugins);

    public Task<IReadOnlyList<ProviderInfo>> GetProviderPluginsAsync(
        CancellationToken cancellationToken = default) =>
        Task.FromResult(ProviderPlugins);

    public Task<IReadOnlyList<McpPluginInfo>> GetMcpsAsync(
        CancellationToken cancellationToken = default) =>
        Task.FromResult(McpPlugins);

    public Task TogglePluginAsync(
        string name,
        bool disabled,
        CancellationToken cancellationToken = default)
    {
        PluginToggles.Add((name, disabled));
        // The production call site discards this task, so a hook failure must
        // fault the task rather than throw into the discard.
        try
        {
            OnTogglePlugin?.Invoke(name, disabled);
        }
        catch (Exception e)
        {
            return Task.FromException(e);
        }

        return Task.CompletedTask;
    }

    public Task ToggleCapabilityAsync(
        string plugin,
        string capability,
        CapabilityFacet facet,
        bool disabled,
        CancellationToken cancellationToken = default)
    {
        CapabilityToggles.Add((plugin, capability, facet, disabled));
        return Task.CompletedTask;
    }

    public Task AddExtensionPluginAsync(
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        OnAddExtensionPlugin?.Invoke(config);
        return Task.CompletedTask;
    }

    public Task AddProviderPluginAsync(
        Plugin config,
        CancellationToken cancellationToken = default) =>
        Task.CompletedTask;

    public Task<McpOauthSession?> InitMcpConnectionAsync(
        Plugin config,
        CancellationToken cancellationToken = default) =>
        Task.FromResult<McpOauthSession?>(null);

    public Task FinalizeMcpConnectionAsync(
        Plugin config,
        McpOauthSession? session,
        CancellationToken cancellationToken = default)
    {
        ThrowIfCancelled(cancellationToken);
        FinalizedMcps.Add(config);
        return Task.CompletedTask;
    }

    public Task UpdatePluginAsync(
        PluginType kind,
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        UpdatedPlugins.Add((kind, config));
        return Task.CompletedTask;
    }

    public Task RemovePluginAsync(
        PluginType kind,
        string name,
        CancellationToken cancellationToken = default)
    {
        OnRemovePlugin?.Invoke(kind, name);
        return Task.CompletedTask;
    }

    public Task<IReadOnlyList<Permission>> GetPermissionsAsync(
        CancellationToken cancellationToken = default) =>
        Task.FromResult(Permissions);

    public Task DeletePermissionAsync(
        string prefix,
        CancellationToken cancellationToken = default)
    {
        OnDeletePermission?.Invoke(prefix);
        DeletedPermissions.Add(prefix);
        return Task.CompletedTask;
    }
}