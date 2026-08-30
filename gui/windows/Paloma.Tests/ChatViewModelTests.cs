using System.Runtime.CompilerServices;
using Paloma.Models;
using Paloma.ViewModels.Overlay;
using Xunit;
using Xunit.Abstractions;
using AllowOnce = PalomaCore.UserDecision.AllowOnce;
using AllowSession = PalomaCore.UserDecision.AllowSession;
using Deny = PalomaCore.UserDecision.Deny;
using IgnorePermission = PalomaCore.UserDecision.IgnorePermission;
using PermissionState = PalomaCore.PermissionState;
using ProviderBackendId = PalomaCore.ProviderBackendId;
using UserDecision = PalomaCore.UserDecision;

namespace Paloma.Tests;

public sealed class ChatViewModelTests(ITestOutputHelper output)
{
    private sealed class FlushGate
    {
        private Action? _pending;

        public bool Defer(Action flush)
        {
            _pending = flush;
            return true;
        }

        public void Run()
        {
            var pending = _pending;
            _pending = null;
            pending?.Invoke();
        }
    }

    [Fact]
    public async Task Streaming_TextMaterialization_Measured()
    {
        const int deltas = 2000;
        var delta = new string('x', 48);

        async Task<long> RunAsync(bool batched)
        {
            var gate = new FlushGate();
            var mock = new MockPalomaClient
            {
                OnChat = (_, _) => Stream()
            };

            async IAsyncEnumerable<ChatStreamEvent> Stream()
            {
                yield return new ChatStreamEvent.SessionStarted("s");
                for (var i = 1; i <= deltas; i++)
                {
                    yield return new ChatStreamEvent.TextDelta(Backend("gpt"), delta);
                    if (batched && i % 16 == 0)
                    {
                        // A dispatcher pass: one coalesced flush per burst.
                        gate.Run();
                    }
                }

                yield return new ChatStreamEvent.Done();
            }

            var vm = new ChatViewModel(mock, batched ? gate.Defer : _ => false);
            var before = GC.GetAllocatedBytesForCurrentThread();
            await vm.SubmitAsync("go");
            gate.Run();
            var bytes = GC.GetAllocatedBytesForCurrentThread() - before;
            var section = Assert.IsType<AssistantSectionViewModel>(vm.Sections[^1]);
            Assert.Equal(deltas * delta.Length, section.Text.Length);
            return bytes;
        }

        await RunAsync(batched: true);
        var immediate = await RunAsync(batched: false);
        var coalesced = await RunAsync(batched: true);

        output.WriteLine($"per-delta materialization: {immediate / 1048576.0:N1} MB allocated");
        output.WriteLine($"per-pass (16 deltas) flush: {coalesced / 1048576.0:N1} MB allocated");
        output.WriteLine($"reduction: {(double)immediate / coalesced:N1}x");
        Assert.True(coalesced < immediate / 4);
    }

    [Fact]
    public async Task Restore_SupersededByAnotherRestore_KeepsNewSessionId()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gateA = new TaskCompletionSource();
        mock.OnRestore = id => id == "A"
            ? Held()
            : MockPalomaClient.Stream<ChatStreamEvent>();

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            // Completes with the exception a cancelled stream surfaces.
            await gateA.Task;
            yield break;
        }

        var restoreA = vm.RestoreAsync("A");
        await vm.RestoreAsync("B");
        gateA.SetException(new InvalidOperationException("superseded"));
        await restoreA;

        await vm.SubmitAsync("hello");

        // The prompt typed under B's transcript must continue session B, not
        // silently fork a fresh session.
        Assert.Equal("B", Assert.Single(mock.ChatCalls).SessionId);
    }

    [Fact]
    public async Task Enter_WithStaleDecisionCursor_FallsThroughToSubmit()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            yield return new ChatStreamEvent.ToolCall(
                "shell",
                "{}",
                null,
                [
                    new AllowOnce("c1"),
                    new AllowSession("s", "c1"),
                ]);
            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 1);
        var section = Assert.IsType<ToolSectionViewModel>(Assert.Single(vm.Sections));

        vm.Navigate(1);
        section.Decisions[1].Decide();

        // The section resolved through a different decision; Enter must act
        // as a plain submit again, not be swallowed by the stale cursor.
        Assert.False(vm.DecideSelected());

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_WhenRpcFails_KeepsDecisionsAvailable()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecide = _ => throw new InvalidOperationException("core busy");

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            yield return new ChatStreamEvent.ToolCall(
                "shell",
                "{}",
                null,
                [new AllowOnce("c1")]);
            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 1);
        var section = Assert.IsType<ToolSectionViewModel>(Assert.Single(vm.Sections));

        section.Decisions[0].Decide();

        // Core is still waiting for a decision, so a transient failure must
        // leave the buttons available for a retry.
        Assert.True(section.Unresolved);
        Assert.Contains("core busy", vm.StatusMessage);

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_Success_ResolvesAndClearsTheHighlights()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecide = decision =>
            decision is Deny
                ? PermissionState.Deny
                : PermissionState.Allow;

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            yield return new ChatStreamEvent.ToolCall(
                "one",
                "{}",
                null,
                [new AllowOnce("c1")]);
            yield return new ChatStreamEvent.ToolCall(
                "two",
                "{}",
                null,
                [new Deny("c2")]);
            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 2);
        var first = Assert.IsType<ToolSectionViewModel>(vm.Sections[0]);
        var second = Assert.IsType<ToolSectionViewModel>(vm.Sections[1]);
        vm.Navigate(1);
        Assert.True(first.Decisions[0].IsSelected);

        first.Decisions[0].Decide();
        second.Decisions[0].Decide();

        Assert.Equal(PermissionState.Allow, first.Resolution);
        Assert.Equal(PermissionState.Deny, second.Resolution);
        Assert.False(first.Unresolved);
        // The decided option retires its own highlight, and the resolved
        // section releases the cursor so enter is a plain submit again.
        Assert.False(first.Decisions[0].IsSelected);
        Assert.False(vm.DecideSelected());

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_AfterResolution_DoesNotFireASecondRpc()
    {
        var calls = 0;
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecide = _ =>
        {
            calls++;
            return PermissionState.Allow;
        };

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            yield return new ChatStreamEvent.ToolCall(
                "shell",
                "{}",
                null,
                [new AllowOnce("c1")]);
            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 1);
        var section = Assert.IsType<ToolSectionViewModel>(Assert.Single(vm.Sections));

        section.Decisions[0].Decide();
        section.Decisions[0].Decide();

        // The section gate stops the second activation before its rpc starts.
        Assert.Equal(1, calls);
        Assert.Equal(PermissionState.Allow, section.Resolution);

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_ErrorState_ResolvesInsteadOfInvitingRetry()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecide = _ => PermissionState.Error;

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            yield return new ChatStreamEvent.ToolCall(
                "shell",
                "{}",
                null,
                [new AllowOnce("c1")]);
            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 1);
        var section = Assert.IsType<ToolSectionViewModel>(Assert.Single(vm.Sections));

        section.Decisions[0].Decide();

        // Core consumed the decision and failed the tool call; nothing is
        // left to retry, so the section resolves like allow and deny.
        Assert.Equal(PermissionState.Error, section.Resolution);
        Assert.False(section.Unresolved);

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_IgnorePermission_ResolvesEveryPendingSectionWithTheOption()
    {
        var decided = new List<UserDecision>();
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecide = decision =>
        {
            decided.Add(decision);
            return PermissionState.Allow;
        };

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            yield return new ChatStreamEvent.ToolCall("one", "{}", null,
            [
                new AllowOnce("c1"),
                new IgnorePermission("s", "c1"),
            ]);
            yield return new ChatStreamEvent.ToolCall("two", "{}", null,
            [
                new AllowOnce("c2"),
                new IgnorePermission("s", "c2"),
            ]);
            yield return new ChatStreamEvent.ToolCall("three", "{}", null,
                [new AllowOnce("c3")]);
            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 3);
        var sections = vm.Sections.OfType<ToolSectionViewModel>().ToList();

        sections[0].Decisions[1].Decide();

        Assert.Equal(PermissionState.Allow, sections[0].Resolution);
        Assert.Equal(PermissionState.Allow, sections[1].Resolution);
        // The section without the option keeps waiting for its own answer.
        Assert.True(sections[2].Unresolved);
        Assert.Equal(["c1", "c2"], decided.Select(d => ((IgnorePermission)d).CallId));

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_IgnorePermission_CascadeDecidesEachSectionOnce()
    {
        var decided = new List<string>();
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecide = decision =>
        {
            decided.Add(((IgnorePermission)decision).CallId);
            return PermissionState.Allow;
        };

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            for (var i = 1; i <= 5; i++)
            {
                yield return new ChatStreamEvent.ToolCall($"tool{i}", "{}", null,
                [
                    new AllowOnce($"c{i}"),
                    new IgnorePermission("s", $"c{i}"),
                ]);
            }

            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 5);
        var sections = vm.Sections.OfType<ToolSectionViewModel>().ToList();

        // Every fanned-out resolution re-enters the fan-out; the resolved
        // and in-flight checks must stop it from deciding anything twice.
        sections[0].Decisions[1].Decide();

        Assert.All(sections, section => Assert.Equal(PermissionState.Allow, section.Resolution));
        Assert.Equal(["c1", "c2", "c3", "c4", "c5"], decided);

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_IgnorePermission_InFlightSectionsAreNotDecidedTwice()
    {
        var calls = new List<string>();
        var pending = new Dictionary<string, TaskCompletionSource<PermissionState>>();
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecideAsync = decision =>
        {
            var id = ((IgnorePermission)decision).CallId;
            calls.Add(id);
            var source = new TaskCompletionSource<PermissionState>(
                TaskCreationOptions.RunContinuationsAsynchronously);
            pending[id] = source;
            return source.Task;
        };

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            for (var i = 1; i <= 3; i++)
            {
                yield return new ChatStreamEvent.ToolCall($"tool{i}", "{}", null,
                [
                    new IgnorePermission("s", $"c{i}"),
                ]);
            }

            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 3);
        var sections = vm.Sections.OfType<ToolSectionViewModel>().ToList();

        sections[0].Decisions[0].Decide();
        Assert.Equal(["c1"], calls);

        // The first resolution fans out to both waiting sections at once.
        pending["c1"].SetResult(PermissionState.Allow);
        await TestWait.UntilAsync(() => calls.Count == 3);

        // The second resolution cascades while the third is still in
        // flight; the deciding gate must not send its rpc again.
        pending["c2"].SetResult(PermissionState.Allow);
        await TestWait.UntilAsync(() => sections[1].Resolution is not null);
        Assert.Equal(["c1", "c2", "c3"], calls);

        pending["c3"].SetResult(PermissionState.Allow);
        await TestWait.UntilAsync(() => sections[2].Resolution is not null);
        Assert.Equal(["c1", "c2", "c3"], calls);
        Assert.All(sections, section => Assert.Equal(PermissionState.Allow, section.Resolution));

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_AllowOnce_LeavesOtherSectionsPending()
    {
        var calls = 0;
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecide = _ =>
        {
            calls++;
            return PermissionState.Allow;
        };

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            yield return new ChatStreamEvent.ToolCall("one", "{}", null,
            [
                new AllowOnce("c1"),
                new IgnorePermission("s", "c1"),
            ]);
            yield return new ChatStreamEvent.ToolCall("two", "{}", null,
            [
                new AllowOnce("c2"),
                new IgnorePermission("s", "c2"),
            ]);
            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 2);
        var sections = vm.Sections.OfType<ToolSectionViewModel>().ToList();

        sections[0].Decisions[0].Decide();

        Assert.Equal(PermissionState.Allow, sections[0].Resolution);
        Assert.True(sections[1].Unresolved);
        Assert.Equal(1, calls);

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Decide_WhenRpcFails_KeepsCursorVisibleForRetry()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();
        mock.OnDecide = _ => throw new InvalidOperationException("core busy");

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            yield return new ChatStreamEvent.ToolCall(
                "shell",
                "{}",
                null,
                [new AllowOnce("c1")]);
            await gate.Task;
        }

        var submit = vm.SubmitAsync("run");
        await TestWait.UntilAsync(() => vm.Sections.Count == 1);
        var section = Assert.IsType<ToolSectionViewModel>(Assert.Single(vm.Sections));
        vm.Navigate(1);
        var decision = section.Decisions[0];
        Assert.True(decision.IsSelected);

        Assert.True(vm.DecideSelected());

        // The decision failed and is still pending; the highlight must stay
        // on it so the user can see what Enter will retry.
        Assert.True(decision.IsSelected);
        Assert.True(vm.DecideSelected());

        gate.SetResult();
        await submit;
    }

    [Fact]
    public async Task Deltas_MergeIntoTheTrailingAssistantSection()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        mock.OnChat = (_, _) => MockPalomaClient.Stream<ChatStreamEvent>(
            new ChatStreamEvent.SessionStarted("s"),
            new ChatStreamEvent.TextDelta(Backend("gpt"), "Hello, "),
            new ChatStreamEvent.TextDelta(Backend("gpt"), "world."),
            new ChatStreamEvent.Done());

        await vm.SubmitAsync("hi");

        var section = Assert.IsType<AssistantSectionViewModel>(Assert.Single(vm.Sections));
        Assert.Equal(("Hello, world.", Backend("gpt")), (section.Text, section.Backend));
    }

    [Fact]
    public async Task Deltas_InterleavedWithReasoning_SplitIntoSections()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        mock.OnChat = (_, _) => MockPalomaClient.Stream<ChatStreamEvent>(
            new ChatStreamEvent.SessionStarted("s"),
            new ChatStreamEvent.TextDelta(Backend("gpt"), "before"),
            new ChatStreamEvent.ReasoningDelta("thinking"),
            new ChatStreamEvent.TextDelta(Backend("gpt"), "after"),
            new ChatStreamEvent.Done());

        await vm.SubmitAsync("hi");

        Assert.Equal(3, vm.Sections.Count);
        Assert.IsType<AssistantSectionViewModel>(vm.Sections[0]);
        Assert.IsType<ReasoningSectionViewModel>(vm.Sections[1]);
        Assert.Equal("after", Assert.IsType<AssistantSectionViewModel>(vm.Sections[2]).Text);
    }

    [Fact]
    public async Task UserPromptEvent_AddsAUserSectionAndBreaksTheMerge()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        mock.OnChat = (_, _) => MockPalomaClient.Stream<ChatStreamEvent>(
            new ChatStreamEvent.SessionStarted("s"),
            new ChatStreamEvent.UserPrompt("hello"),
            new ChatStreamEvent.TextDelta(Backend("gpt"), "hi"),
            new ChatStreamEvent.Done());

        await vm.SubmitAsync("hello");

        Assert.Equal(2, vm.Sections.Count);
        Assert.Equal("hello", Assert.IsType<UserSectionViewModel>(vm.Sections[0]).Text);
        Assert.Equal("hi", Assert.IsType<AssistantSectionViewModel>(vm.Sections[1]).Text);
    }

    [Fact]
    public async Task CancelEvent_MarksTheStatusCancelled()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        mock.OnChat = (_, _) => MockPalomaClient.Stream<ChatStreamEvent>(
            new ChatStreamEvent.SessionStarted("s"),
            new ChatStreamEvent.TextDelta(Backend("gpt"), "partial"),
            new ChatStreamEvent.Cancelled());

        await vm.SubmitAsync("hi");

        Assert.Equal(ChatStatus.Cancelled, vm.Status);
        Assert.False(vm.Streaming);
    }

    [Fact]
    public async Task ErrorEvent_FailsWithTheStreamedMessage()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        mock.OnChat = (_, _) => MockPalomaClient.Stream<ChatStreamEvent>(
            new ChatStreamEvent.SessionStarted("s"),
            new ChatStreamEvent.Error("provider exploded"));

        await vm.SubmitAsync("hi");

        Assert.Equal(ChatStatus.Failed, vm.Status);
        Assert.Equal("provider exploded", vm.StatusMessage);
    }

    [Fact]
    public async Task StreamEndingWithoutTerminalEvent_FallsBackToIdle()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        mock.OnChat = (_, _) => MockPalomaClient.Stream<ChatStreamEvent>(
            new ChatStreamEvent.SessionStarted("s"),
            new ChatStreamEvent.TextDelta(Backend("gpt"), "hi"));

        await vm.SubmitAsync("hi");

        // The spinner must not run forever on a stream that just closes.
        Assert.Equal(ChatStatus.Idle, vm.Status);
    }

    [Fact]
    public async Task Submit_WithNoModelSelected_FailsWithGuidance()
    {
        var mock = new MockPalomaClient { PreferredBackend = null };
        var vm = new ChatViewModel(mock);

        await vm.SubmitAsync("hi");

        Assert.Equal(ChatStatus.Failed, vm.Status);
        Assert.Equal("No model selected. Connect a provider first.", vm.StatusMessage);
        Assert.Empty(mock.ChatCalls);
    }

    [Fact]
    public async Task Navigate_WalksInOrderClampsAtTheEndAndExitsAboveTheFirst()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        mock.OnChat = (_, _) => MockPalomaClient.Stream<ChatStreamEvent>(
            new ChatStreamEvent.SessionStarted("s"),
            new ChatStreamEvent.ToolCall(
                "one",
                "{}",
                null,
                [
                    new AllowOnce("c1"),
                    new Deny("c1"),
                ]),
            new ChatStreamEvent.ToolCall(
                "two",
                "{}",
                null,
                [new AllowOnce("c2")]),
            new ChatStreamEvent.Done());
        await vm.SubmitAsync("run");
        var first = Assert.IsType<ToolSectionViewModel>(vm.Sections[0]);
        var second = Assert.IsType<ToolSectionViewModel>(vm.Sections[1]);

        // Down walks the decisions in the core's order, across sections.
        Assert.Same(first, vm.Navigate(1));
        Assert.True(first.Decisions[0].IsSelected);
        Assert.Same(first, vm.Navigate(1));
        Assert.True(first.Decisions[1].IsSelected);
        Assert.False(first.Decisions[0].IsSelected);
        Assert.Same(second, vm.Navigate(1));
        Assert.True(second.Decisions[0].IsSelected);

        // Down at the last decision stays put.
        Assert.Same(second, vm.Navigate(1));
        Assert.True(second.Decisions[0].IsSelected);

        // Up walks back, and above the first returns to the plain input.
        Assert.Same(first, vm.Navigate(-1));
        Assert.Same(first, vm.Navigate(-1));
        Assert.Null(vm.Navigate(-1));
        Assert.False(first.Decisions[0].IsSelected);
        Assert.False(vm.DecideSelected());
    }

    [Fact]
    public async Task CanSubmit_BlankOrStreaming_IsFalse()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            await gate.Task;
        }

        Assert.False(vm.CanSubmit("   "));
        Assert.True(vm.CanSubmit("prompt"));

        var turn = vm.SubmitAsync("prompt");
        await TestWait.UntilAsync(() => vm.Streaming);
        Assert.False(vm.CanSubmit("prompt"));

        gate.SetResult();
        await turn;
    }

    [Fact]
    public async Task Submit_WhileStreaming_DoesNotStartSecondTurn()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var gate = new TaskCompletionSource();
        mock.OnChat = (_, _) => Held();

        async IAsyncEnumerable<ChatStreamEvent> Held()
        {
            yield return new ChatStreamEvent.SessionStarted("s");
            await gate.Task;
        }

        var first = vm.SubmitAsync("first");
        await TestWait.UntilAsync(() => vm.Streaming);

        await vm.SubmitAsync("second");

        Assert.Single(mock.ChatCalls);

        gate.SetResult();
        await first;
    }

    [Fact]
    public async Task Submit_EventsAfterDone_NeverReachTheViewModel()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        mock.OnChat = (_, _) => MockPalomaClient.Stream<ChatStreamEvent>(
            new ChatStreamEvent.SessionStarted("s"),
            new ChatStreamEvent.Done(),
            new ChatStreamEvent.TextDelta(Backend("b"), "late"));

        await vm.SubmitAsync("hello");

        // The mock cuts the stream after Done the way the real client does.
        Assert.Equal(ChatStatus.Idle, vm.Status);
        Assert.Empty(vm.Sections);
    }

    [Fact]
    public async Task Interrupt_BeforeSessionStarted_StillStopsTheTurn()
    {
        var mock = new MockPalomaClient();
        var vm = new ChatViewModel(mock);
        var started = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);
        mock.OnChat = (_, _) => HangWithoutSession();

        async IAsyncEnumerable<ChatStreamEvent> HangWithoutSession(
            [EnumeratorCancellation] CancellationToken token = default)
        {
            started.SetResult();
            // The provider hangs while connecting: SessionStarted never arrives.
            await Task.Delay(Timeout.Infinite, token);
            yield break;
        }

        var turn = vm.SubmitAsync("hello");
        await started.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.True(vm.Streaming);

        await vm.InterruptAsync();

        // No session id is known yet; the interrupt falls back to local
        // cancellation instead of doing nothing.
        Assert.False(vm.Streaming);
        Assert.Equal(ChatStatus.Cancelled, vm.Status);
        Assert.Empty(mock.CancelledSessions);
        await turn.WaitAsync(TimeSpan.FromSeconds(5));
    }

    private static ProviderBackendId Backend(string id)
    {
        return new ProviderBackendId("prov", id);
    }
}