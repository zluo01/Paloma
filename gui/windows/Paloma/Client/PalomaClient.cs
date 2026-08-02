using System.IO.Pipes;
using System.Runtime.CompilerServices;
using System.Security.Principal;
using Grpc.Core;
using Grpc.Net.Client;
using Paloma.Binding.V1;
using Paloma.Extension.V1;
using Paloma.Models;
using Paloma.Provider.Runtime.V1;
using BindingRpc = Paloma.Binding.V1.Binding;
using CancelConnectionRequest = Paloma.Binding.V1.CancelConnectionRequest;
using ChatRequest = Paloma.Binding.V1.ChatRequest;
using ExtAction = Paloma.Extension.V1.Action;
using FinalizeConnectionRequest = Paloma.Binding.V1.FinalizeConnectionRequest;
using InitConnectionRequest = Paloma.Binding.V1.InitConnectionRequest;
using ProviderAuthMethod = Paloma.Provider.Runtime.V1.ProviderAuthMethod;
using SearchRequest = Paloma.Binding.V1.SearchRequest;

namespace Paloma.Client;

public sealed partial class PalomaClient : IPalomaClient, IDisposable
{
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(3);
    private static readonly TimeSpan StartupHealthTimeout = TimeSpan.FromSeconds(5);

    private readonly GrpcChannel? _channel;
    private readonly BindingRpc.BindingClient _client;

    public PalomaClient(string pipeName)
    {
        var handler = new SocketsHttpHandler
        {
            ConnectCallback = (_, cancellationToken) => ConnectAsync(pipeName, cancellationToken),
            ConnectTimeout = ConnectTimeout,
        };
        // The address is required but never used.
        // Every connection goes through ConnectCallback to the named pipe.
        _channel = GrpcChannel.ForAddress(
            "http://localhost",
            new GrpcChannelOptions { HttpHandler = handler });
        _client = new BindingRpc.BindingClient(_channel);
        // Block wait for health check on startup, grpc service is required to be healthy.
        _client.Health(new HealthRequest(), deadline: DateTime.UtcNow.Add(StartupHealthTimeout));
    }

    internal PalomaClient(BindingRpc.BindingClient client)
    {
        _client = client;
    }

    public static string Describe(Exception e)
    {
        return e is RpcException { Status.Detail.Length: > 0 } rpc
            ? rpc.Status.Detail
            : e.Message;
    }

    public static bool IsCancellation(Exception e)
    {
        return e is OperationCanceledException
            or RpcException { StatusCode: StatusCode.Cancelled };
    }

    public async IAsyncEnumerable<QueryResponse> SearchAsync(
        string input,
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        using var call = _client.Search(
            new SearchRequest { Input = input },
            cancellationToken: cancellationToken);
        await foreach (var e in call.ResponseStream.ReadAllAsync(cancellationToken))
        {
            if (e.PayloadCase != RenderEvent.PayloadOneofCase.Search)
            {
                if (e.PayloadCase is RenderEvent.PayloadOneofCase.Done
                    or RenderEvent.PayloadOneofCase.Cancel)
                {
                    yield break;
                }

                // Error events here are per-extension search failures;
                // search is best-effort, so they drop instead of ending
                // the stream.
                continue;
            }

            if (e.Search.PayloadCase != SearchRenderEvent.PayloadOneofCase.Append)
            {
                continue;
            }

            yield return e.Search.Append;
        }
    }

    public async Task<RunActionResponse?> RunSearchActionAsync(
        ExtensionCapabilityId capabilityId,
        ExtAction action,
        CancellationToken cancellationToken = default)
    {
        var request = new RunSearchActionRequest
        {
            ExtensionCapabilityId = capabilityId,
            Action = action,
        };
        var response = await _client.RunSearchActionAsync(
            request,
            cancellationToken: cancellationToken);
        return response.Behavior;
    }

    public async Task<ProviderBackendId?> PreferModelAsync(CancellationToken cancellationToken = default)
    {
        var response = await _client.PreferModelAsync(
            new PreferModelRequest(),
            cancellationToken: cancellationToken);
        return response.ProviderBackendId;
    }

    public async IAsyncEnumerable<ChatStreamEvent> ChatAsync(
        string? sessionId,
        ProviderBackendId backend,
        string prompt,
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        var request = new ChatRequest
        {
            ProviderBackendId = backend,
            Prompt = prompt,
        };
        if (sessionId is not null)
        {
            request.SessionId = sessionId;
        }

        using var call = _client.Chat(request, cancellationToken: cancellationToken);
        await foreach (var e in call.ResponseStream.ReadAllAsync(cancellationToken))
        {
            var mapped = e.PayloadCase switch
            {
                ChatEvent.PayloadOneofCase.SessionStarted =>
                    new ChatStreamEvent.SessionStarted(e.SessionStarted),
                ChatEvent.PayloadOneofCase.Event => MapRenderEvent(e.Event),
                _ => null,
            };
            if (mapped is null) continue;
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
        var response = await _client.AvailableSessionsAsync(
            new AvailableSessionsRequest(),
            cancellationToken: cancellationToken);
        return [.. response.Sessions];
    }

    public async Task<IReadOnlyList<string>> SearchSessionsAsync(
        string needle,
        CancellationToken cancellationToken = default)
    {
        var response = await _client.SearchSessionsAsync(
            new SearchSessionsRequest { Needle = needle },
            cancellationToken: cancellationToken);
        return [.. response.SessionIds];
    }

    public async Task RemoveSessionAsync(
        string sessionId,
        CancellationToken cancellationToken = default)
    {
        await _client.RemoveSessionAsync(
            new RemoveSessionRequest { SessionId = sessionId },
            cancellationToken: cancellationToken);
    }

    public async IAsyncEnumerable<ChatStreamEvent> RestoreSessionAsync(
        string sessionId,
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        using var call = _client.RestoreSession(
            new RestoreSessionRequest { SessionId = sessionId },
            cancellationToken: cancellationToken);
        await foreach (var e in call.ResponseStream.ReadAllAsync(cancellationToken))
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
        await _client.CancelSessionAsync(
            new CancelSessionRequest { SessionId = sessionId },
            cancellationToken: cancellationToken);
    }

    public async Task<PermissionState> DecideAsync(
        UserDecision decision,
        CancellationToken cancellationToken = default)
    {
        var response = await _client.DecideToolcallPermissionsAsync(
            new DecideToolcallPermissionsRequest { UserDecision = decision },
            cancellationToken: cancellationToken);
        return response.State;
    }

    public async Task<(HealthLevel Services, HealthLevel Plugins)> GetHealthAsync(
        CancellationToken cancellationToken = default)
    {
        var services = _client.ConnectorsHealthLevelAsync(
            new ConnectorsHealthLevelRequest(),
            cancellationToken: cancellationToken).ResponseAsync;
        var plugins = _client.PluginsHealthLevelAsync(
            new PluginsHealthLevelRequest(),
            cancellationToken: cancellationToken).ResponseAsync;
        await Task.WhenAll(services, plugins);
        return ((await services).HealthLevel, (await plugins).HealthLevel);
    }

    public async Task<IReadOnlyList<Connector>> GetConnectorsAsync(
        CancellationToken cancellationToken = default)
    {
        var response = await _client.AvailableConnectorsAsync(
            new AvailableConnectorsRequest(),
            cancellationToken: cancellationToken);
        return response.Connectors;
    }

    public async Task<ConnectionPhase> InitConnectionAsync(
        ProviderBackendId id,
        CancellationToken cancellationToken = default)
    {
        var response = await _client.InitConnectionAsync(
            new InitConnectionRequest { ProviderBackendId = id },
            cancellationToken: cancellationToken);
        var payload = response.Connection;
        return payload?.PayloadCase switch
        {
            ConnectionPayload.PayloadOneofCase.DeviceCode =>
                new ConnectionPhase.Challenge(payload.DeviceCode),
            ConnectionPayload.PayloadOneofCase.BrowserRedirect =>
                new ConnectionPhase.Oauth(payload.BrowserRedirect),
            ConnectionPayload.PayloadOneofCase.ManualInput =>
                new ConnectionPhase.Manual(payload.ManualInput),
            _ => new ConnectionPhase.Failed("provider returned an empty connection payload"),
        };
    }

    public async Task FinalizeConnectionAsync(
        ProviderBackendId id,
        ProviderAuthMethod method,
        string payload,
        CancellationToken cancellationToken = default)
    {
        await _client.FinalizeConnectionAsync(
            new FinalizeConnectionRequest
            {
                ProviderAuthMethod = method,
                ProviderBackendId = id,
                Payload = payload,
            },
            cancellationToken: cancellationToken);
    }

    public async Task CancelConnectionAsync(
        ProviderBackendId id,
        CancellationToken cancellationToken = default)
    {
        await _client.CancelConnectionAsync(
            new CancelConnectionRequest { ProviderBackendId = id },
            cancellationToken: cancellationToken);
    }

    public async Task DisconnectAsync(ProviderBackendId id, CancellationToken cancellationToken = default)
    {
        await _client.DisconnectConnectorAsync(
            new DisconnectConnectorRequest { ProviderBackendId = id },
            cancellationToken: cancellationToken);
    }

    public async Task SetModelPreferenceAsync(
        ProviderBackendId id,
        string model,
        string effort,
        bool asDefault = false,
        CancellationToken cancellationToken = default)
    {
        await _client.SetModelPreferenceAsync(
            new SetModelPreferenceRequest
            {
                ProviderBackendId = id,
                Model = model,
                Effort = effort,
                AsDefault = asDefault,
            },
            cancellationToken: cancellationToken);
    }

    public async Task<IReadOnlyList<ExtensionInfo>> GetExtensionPluginsAsync(
        CancellationToken cancellationToken = default)
    {
        var response = await _client.ListExtensionPluginsAsync(
            new ListExtensionPluginsRequest(),
            cancellationToken: cancellationToken);
        return response.Extensions;
    }

    public async Task<IReadOnlyList<ProviderInfo>> GetProviderPluginsAsync(
        CancellationToken cancellationToken = default)
    {
        var response = await _client.ListProviderPluginsAsync(
            new ListProviderPluginsRequest(),
            cancellationToken: cancellationToken);
        return response.Providers;
    }

    public async Task<IReadOnlyList<McpPluginInfo>> GetMcpsAsync(
        CancellationToken cancellationToken = default)
    {
        var response = await _client.ListMcpsAsync(
            new ListMcpsRequest(),
            cancellationToken: cancellationToken);
        return response.Mcps;
    }

    public async Task TogglePluginAsync(
        string name,
        bool disabled,
        CancellationToken cancellationToken = default)
    {
        await _client.TogglePluginAsync(
            new TogglePluginRequest { Name = name, Disabled = disabled },
            cancellationToken: cancellationToken);
    }

    public async Task AddExtensionPluginAsync(
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        await _client.AddExtensionPluginAsync(
            new AddExtensionPluginRequest { Config = config },
            cancellationToken: cancellationToken);
    }

    public async Task AddProviderPluginAsync(
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        await _client.AddProviderPluginAsync(
            new AddProviderPluginRequest { Config = config },
            cancellationToken: cancellationToken);
    }

    public async Task<(string? SessionId, string? AuthUrl)> InitMcpConnectionAsync(
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        var response = await _client.InitMcpConnectionAsync(
            new InitMcpConnectionRequest { Config = config },
            cancellationToken: cancellationToken);
        return (
            response.HasOauthSessionId ? response.OauthSessionId : null,
            response.HasAuthUrl ? response.AuthUrl : null);
    }

    public async Task FinalizeMcpConnectionAsync(
        Plugin config,
        string? oauthSessionId,
        CancellationToken cancellationToken = default)
    {
        var request = new FinalizeMcpConnectionRequest { Config = config };
        if (oauthSessionId is not null)
        {
            request.OauthSessionId = oauthSessionId;
        }

        await _client.FinalizeMcpConnectionAsync(request, cancellationToken: cancellationToken);
    }

    public async Task UpdatePluginAsync(
        PluginType kind,
        Plugin config,
        CancellationToken cancellationToken = default)
    {
        await _client.UpdatePluginAsync(
            new UpdatePluginRequest { PluginType = kind, Plugin = config },
            cancellationToken: cancellationToken);
    }

    public async Task RemovePluginAsync(
        PluginType kind,
        string name,
        CancellationToken cancellationToken = default)
    {
        await _client.RemovePluginAsync(
            new RemovePluginRequest { PluginType = kind, Name = name },
            cancellationToken: cancellationToken);
    }

    public async Task<IReadOnlyList<Permission>> GetPermissionsAsync(
        CancellationToken cancellationToken = default)
    {
        var response = await _client.GetPermissionsAsync(
            new GetPermissionsRequest(),
            cancellationToken: cancellationToken);
        return response.Permissions;
    }

    public async Task DeletePermissionAsync(
        string prefix,
        CancellationToken cancellationToken = default)
    {
        await _client.DeletePermissionAsync(
            new DeletePermissionRequest { Prefix = prefix },
            cancellationToken: cancellationToken);
    }

    public async Task ToggleCapabilityAsync(
        string plugin,
        string capability,
        CapabilityFacet facet,
        bool disabled,
        CancellationToken cancellationToken = default)
    {
        await _client.ToggleCapabilityAsync(
            new ToggleCapabilityRequest
            {
                Name = plugin,
                Capability = capability,
                Facet = facet,
                Disabled = disabled,
            },
            cancellationToken: cancellationToken);
    }

    public void Dispose()
    {
        _channel?.Dispose();
    }

    private static async ValueTask<Stream> ConnectAsync(
        string pipeName,
        CancellationToken cancellationToken)
    {
        var pipe = new NamedPipeClientStream(
            ".",
            pipeName,
            PipeDirection.InOut,
            PipeOptions.Asynchronous,
            TokenImpersonationLevel.None);
        try
        {
            await pipe.ConnectAsync((int)ConnectTimeout.TotalMilliseconds, cancellationToken);
            ValidateOwner(pipe);
            return pipe;
        }
        catch
        {
            await pipe.DisposeAsync();
            throw;
        }
    }

    /// Any local account could create a fake pipe under the same name.
    /// Only talk to a pipe owned by the current user.
    private static void ValidateOwner(NamedPipeClientStream pipe)
    {
        var owner = pipe.GetAccessControl().GetOwner(typeof(SecurityIdentifier));
        using var identity = WindowsIdentity.GetCurrent();
        var current = identity.User;
        if (current is null || !current.Equals(owner))
        {
            throw new UnauthorizedAccessException("the core pipe is not owned by the current user");
        }
    }

    private static ChatStreamEvent? MapRenderEvent(RenderEvent e)
    {
        return e.PayloadCase switch
        {
            RenderEvent.PayloadOneofCase.Chat => e.Chat.PayloadCase switch
            {
                ChatRenderEvent.PayloadOneofCase.UserPrompt =>
                    new ChatStreamEvent.UserPrompt(e.Chat.UserPrompt),
                ChatRenderEvent.PayloadOneofCase.TextDelta => new ChatStreamEvent.TextDelta(
                    e.Chat.TextDelta.ProviderBackendId,
                    e.Chat.TextDelta.Text),
                ChatRenderEvent.PayloadOneofCase.ReasoningDelta =>
                    new ChatStreamEvent.ReasoningDelta(e.Chat.ReasoningDelta),
                ChatRenderEvent.PayloadOneofCase.ToolCall => new ChatStreamEvent.ToolCall(
                    e.Chat.ToolCall.ToolName,
                    e.Chat.ToolCall.Arguments,
                    e.Chat.ToolCall.HasDescription ? e.Chat.ToolCall.Description : null,
                    [
                        .. e.Chat.ToolCall.Decisions.Where(decision =>
                            decision.DecisionCase != UserDecision.DecisionOneofCase.None)
                    ]),
                _ => null,
            },
            RenderEvent.PayloadOneofCase.Done => new ChatStreamEvent.Done(),
            RenderEvent.PayloadOneofCase.Cancel => new ChatStreamEvent.Cancelled(),
            RenderEvent.PayloadOneofCase.Error => new ChatStreamEvent.Error(e.Error),
            _ => null,
        };
    }

    internal static bool IsTerminal(ChatStreamEvent e)
    {
        return e is ChatStreamEvent.Done or ChatStreamEvent.Cancelled or ChatStreamEvent.Error;
    }
}