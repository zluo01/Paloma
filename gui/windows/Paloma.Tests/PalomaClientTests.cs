using Grpc.Core;
using Paloma.Binding.V1;
using Paloma.Client;
using Paloma.Models;
using Xunit;
using BindingRpc = Paloma.Binding.V1.Binding;
using PbDecision = Paloma.Binding.V1.UserDecision;

namespace Paloma.Tests;

public sealed class PalomaClientTests
{
    private sealed class FakeStreamReader<T>(IEnumerable<T> items) : IAsyncStreamReader<T>
    {
        private readonly IEnumerator<T> _items = items.GetEnumerator();

        public T Current => _items.Current;

        public Task<bool> MoveNext(CancellationToken cancellationToken) =>
            Task.FromResult(_items.MoveNext());
    }

    private sealed class ChatStreamingClient(params ChatEvent[] events) : BindingRpc.BindingClient
    {
        public override AsyncServerStreamingCall<ChatEvent> Chat(
            ChatRequest request,
            CallOptions options) => new(
            new FakeStreamReader<ChatEvent>(events),
            Task.FromResult(new Metadata()),
            () => Status.DefaultSuccess,
            () => new Metadata(),
            () => { });
    }

    [Fact]
    public async Task Chat_UnknownDecisionVariant_IsSkippedNotFatal()
    {
        var grpc = new ChatStreamingClient(new ChatEvent
        {
            Event = new RenderEvent
            {
                Chat = new ChatRenderEvent
                {
                    ToolCall = new ToolCall
                    {
                        ToolName = "shell",
                        Arguments = "{}",
                        Decisions =
                        {
                            new PbDecision { AllowOnce = new AllowOnce { CallId = "c1" } },
                            // a decision variant this client does not know
                            new PbDecision(),
                        },
                    },
                },
            },
        });
        using var client = new PalomaClient(grpc);

        var events = new List<ChatStreamEvent>();
        await foreach (var e in client.ChatAsync(
                           null,
                           new ProviderBackendId { ProviderId = "prov", BackendId = "model" },
                           "hi"))
        {
            events.Add(e);
        }

        var call = Assert.IsType<ChatStreamEvent.ToolCall>(Assert.Single(events));
        var decision = Assert.Single(call.Decisions);
        Assert.Equal(PbDecision.DecisionOneofCase.AllowOnce, decision.DecisionCase);
        Assert.Equal("c1", decision.AllowOnce.CallId);
    }

    private sealed class RecordingBindingClient : BindingRpc.BindingClient
    {
        public TaskCompletionSource<ConnectorsHealthLevelResponse> Services { get; } =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public TaskCompletionSource<PluginsHealthLevelResponse> Plugins { get; } =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public List<string> Issued { get; } = [];

        public override AsyncUnaryCall<ConnectorsHealthLevelResponse> ConnectorsHealthLevelAsync(
            ConnectorsHealthLevelRequest request,
            Metadata? headers = null,
            DateTime? deadline = null,
            CancellationToken cancellationToken = default)
        {
            Issued.Add("services");
            return Call(Services.Task);
        }

        public override AsyncUnaryCall<PluginsHealthLevelResponse> PluginsHealthLevelAsync(
            PluginsHealthLevelRequest request,
            Metadata? headers = null,
            DateTime? deadline = null,
            CancellationToken cancellationToken = default)
        {
            Issued.Add("plugins");
            return Call(Plugins.Task);
        }

        private static AsyncUnaryCall<T> Call<T>(Task<T> response) => new(
            response,
            Task.FromResult(new Metadata()),
            () => Status.DefaultSuccess,
            () => new Metadata(),
            () => { });
    }

    [Fact]
    public async Task GetHealth_IssuesBothRpcsBeforeAwaitingEither()
    {
        var grpc = new RecordingBindingClient();
        using var client = new PalomaClient(grpc);

        var health = client.GetHealthAsync();

        // The two calls are independent and share one multiplexed channel;
        // the summon path must not pay their latencies back to back.
        Assert.Equal(["services", "plugins"], grpc.Issued);

        grpc.Services.SetResult(new ConnectorsHealthLevelResponse { HealthLevel = HealthLevel.Healthy });
        grpc.Plugins.SetResult(new PluginsHealthLevelResponse { HealthLevel = HealthLevel.Down });
        var (services, plugins) = await health;
        Assert.Equal(HealthLevel.Healthy, services);
        Assert.Equal(HealthLevel.Down, plugins);
    }
}