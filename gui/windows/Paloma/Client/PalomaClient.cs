using System.Runtime.CompilerServices;
using Paloma.Models;
using PalomaCore;
using ExtAction = PalomaCore.Action;

namespace Paloma.Client;

public sealed partial class PalomaClient(PalomaApp app) : IPalomaClient, IDisposable
{
    public static string Describe(Exception e)
    {
        return e is PalomaException.Failure failure ? failure.message : e.Message;
    }

    public static bool IsCancellation(Exception e)
    {
        return e is OperationCanceledException or PalomaException.Failure { message: "cancelled" };
    }

    public async IAsyncEnumerable<QueryResponse> SearchAsync(
        string input,
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        using var stream = await app.Search(input);
        await foreach (var e in Events(stream.Next, cancellationToken))
        {
            switch (e)
            {
                case RenderEvent.Search { Event: SearchRenderEvent.Append append }:
                    yield return append.Response;
                    break;
                case RenderEvent.Done or RenderEvent.Cancel:
                    yield break;
            }
        }
    }

    public async Task<Behavior?> RunSearchActionAsync(
        ExtensionCapabilityId capabilityId,
        ExtAction action,
        CancellationToken cancellationToken = default)
    {
        return await app.RunSearchAction(capabilityId, action);
    }

    public async Task<ProviderBackendId?> PreferModelAsync(CancellationToken cancellationToken = default)
    {
        return await app.PreferModel();
    }

    public async IAsyncEnumerable<ChatStreamEvent> ChatAsync(
        string? sessionId,
        ProviderBackendId backend,
        string prompt,
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        using var chat = await app.Chat(sessionId, backend, prompt);
        if (chat.SessionId() is { } started)
        {
            yield return new ChatStreamEvent.SessionStarted(started);
        }

        await foreach (var e in Events(chat.Next, cancellationToken))
        {
            if (MapRenderEvent(e) is not { } mapped) continue;
            yield return mapped;
            if (IsTerminal(mapped))
            {
                yield break;
            }
        }
    }

    public async Task<IReadOnlyList<SessionListItem>> GetSessionsAsync(
        CancellationToken cancellationToken = default)
    {
        return await app.AvailableSessions();
    }

    public async Task<IReadOnlyList<string>> SearchSessionsAsync(
        string needle,
        CancellationToken cancellationToken = default)
    {
        return await app.SearchSessions(needle);
    }

    public async Task RemoveSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
    {
        await app.RemoveSession(sessionId);
    }

    public async IAsyncEnumerable<ChatStreamEvent> RestoreSessionAsync(
        string sessionId,
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        using var stream = await app.RestoreSession(sessionId);
        await foreach (var e in Events(stream.Next, cancellationToken))
        {
            if (MapRenderEvent(e) is not { } mapped) continue;
            yield return mapped;
            if (IsTerminal(mapped))
            {
                yield break;
            }
        }
    }

    public async Task CancelSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
    {
        await app.CancelSession(sessionId);
    }

    public async Task<PermissionState> DecideAsync(
        UserDecision decision,
        CancellationToken cancellationToken = default)
    {
        return await app.DecideToolcallPermissions(decision);
    }

    public async Task<(HealthLevel Services, HealthLevel Plugins)> GetHealthAsync(
        CancellationToken cancellationToken = default)
    {
        var services = app.ConnectorsHealthLevel();
        var plugins = app.PluginsHealthLevel();
        await Task.WhenAll(services, plugins);
        return (await services, await plugins);
    }

    public async Task<IReadOnlyList<Connector>> GetConnectorsAsync(
        CancellationToken cancellationToken = default)
    {
        return await app.AvailableConnectors();
    }

    public async Task<ConnectionPhase> InitConnectionAsync(
        ProviderBackendId id,
        CancellationToken cancellationToken = default)
    {
        return await app.InitConnection(id) switch
        {
            ConnectionPayload.DeviceCode payload => new ConnectionPhase.Challenge(payload),
            ConnectionPayload.BrowserRedirect payload => new ConnectionPhase.Oauth(payload),
            ConnectionPayload.ManualInput payload => new ConnectionPhase.Manual(payload),
            _ => new ConnectionPhase.Failed("provider returned an empty connection payload"),
        };
    }

    public async Task FinalizeConnectionAsync(
        ProviderBackendId id,
        ProviderAuthMethod method,
        string payload,
        CancellationToken cancellationToken = default)
    {
        await app.FinalizeConnection(method, id, payload);
    }

    public async Task CancelConnectionAsync(
        ProviderBackendId id,
        CancellationToken cancellationToken = default)
    {
        await app.CancelConnection(id);
    }

    public async Task DisconnectAsync(ProviderBackendId id, CancellationToken cancellationToken = default)
    {
        await app.DisconnectConnector(id);
    }

    public async Task SetModelPreferenceAsync(
        ProviderBackendId id,
        string model,
        string effort,
        bool asDefault = false,
        CancellationToken cancellationToken = default)
    {
        await app.SetModelPreference(id, model, effort, asDefault);
    }

    public async Task<IReadOnlyList<ExtensionInfo>> GetExtensionPluginsAsync(
        CancellationToken cancellationToken = default)
    {
        return await app.ListExtensionPlugins();
    }

    public async Task<IReadOnlyList<ProviderInfo>> GetProviderPluginsAsync(
        CancellationToken cancellationToken = default)
    {
        return await app.ListProviderPlugins();
    }

    public async Task<IReadOnlyList<McpPluginInfo>> GetMcpsAsync(
        CancellationToken cancellationToken = default)
    {
        return await app.ListMcps();
    }

    public async Task TogglePluginAsync(
        string name,
        bool disabled,
        CancellationToken cancellationToken = default)
    {
        await app.TogglePlugin(name, disabled);
    }

    public async Task AddExtensionPluginAsync(
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        await app.AddExtensionPlugin(config);
    }

    public async Task AddProviderPluginAsync(
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        await app.AddProviderPlugin(config);
    }

    public async Task<McpOauthSession?> InitMcpConnectionAsync(
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        return await app.InitMcpConnection(config);
    }

    public async Task FinalizeMcpConnectionAsync(
        Plugin config,
        McpOauthSession? session,
        CancellationToken cancellationToken = default)
    {
        await app.FinalizeMcpConnection(config, session);
    }

    public async Task UpdatePluginAsync(
        PluginType kind,
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        await app.UpdatePlugin(kind, config);
    }

    public async Task RemovePluginAsync(
        PluginType kind,
        string name,
        CancellationToken cancellationToken = default)
    {
        await app.RemovePlugin(kind, name);
    }

    public async Task<IReadOnlyList<Permission>> GetPermissionsAsync(
        CancellationToken cancellationToken = default)
    {
        return await app.GetPermissions();
    }

    public async Task DeletePermissionAsync(
        string prefix,
        CancellationToken cancellationToken = default)
    {
        await app.DeletePermission(prefix);
    }

    public async Task ToggleCapabilityAsync(
        string plugin,
        string capability,
        CapabilityFacet facet,
        bool disabled,
        CancellationToken cancellationToken = default)
    {
        await app.ToggleCapability(plugin, capability, facet, disabled);
    }

    public void Dispose()
    {
        app.Dispose();
    }

    private static async IAsyncEnumerable<RenderEvent> Events(
        Func<Task<RenderEvent?>> next,
        [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        while (!cancellationToken.IsCancellationRequested && await next() is { } e)
        {
            yield return e;
        }

        cancellationToken.ThrowIfCancellationRequested();
    }

    private static ChatStreamEvent? MapRenderEvent(RenderEvent e)
    {
        return e switch
        {
            RenderEvent.Chat { Event: ChatRenderEvent.UserPrompt prompt } =>
                new ChatStreamEvent.UserPrompt(prompt.Text),
            RenderEvent.Chat { Event: ChatRenderEvent.TextDelta delta } =>
                new ChatStreamEvent.TextDelta(delta.ProviderBackendId, delta.Text),
            RenderEvent.Chat { Event: ChatRenderEvent.ReasoningDelta delta } =>
                new ChatStreamEvent.ReasoningDelta(delta.Text),
            RenderEvent.Chat { Event: ChatRenderEvent.ToolCall call } => new ChatStreamEvent.ToolCall(
                call.ToolName,
                call.Arguments,
                call.Description,
                call.Decisions),
            RenderEvent.Done => new ChatStreamEvent.Done(),
            RenderEvent.Cancel => new ChatStreamEvent.Cancelled(),
            RenderEvent.Error error => new ChatStreamEvent.Error(error.Message),
            _ => null,
        };
    }

    internal static bool IsTerminal(ChatStreamEvent e)
    {
        return e is ChatStreamEvent.Done or ChatStreamEvent.Cancelled or ChatStreamEvent.Error;
    }
}