using Paloma.ViewModels.Overlay;
using Xunit;
using SessionListItem = PalomaCore.SessionListItem;

namespace Paloma.Tests;

public sealed class SessionsViewModelTests
{
    private static readonly string[] RemainingSessions = ["a", "c"];
    private static readonly string[] RetypedNeedles = ["ne", "needle"];

    private static SessionListItem Session(string id) =>
        new(id, $"Session {id}", 0);

    [Fact]
    public async Task Delete_UnderActiveFilter_KeepsFilteredView()
    {
        var mock = new MockPalomaClient
        {
            Sessions = [Session("a"), Session("b"), Session("c")],
            OnSearchSessions = _ => ["a", "b"],
        };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();
        await vm.SearchAsync("needle");
        Assert.Equal(2, vm.Rows.Count);

        await vm.RemoveAsync(vm.Rows[0]);

        // The filter stays applied: only the remaining match is shown, not
        // every stored session under the stale query.
        Assert.Equal("b", Assert.Single(vm.Rows).Item.SessionId);
    }

    [Fact]
    public async Task ConfirmDelete_AnchorsSelectionOnRowAbove()
    {
        var mock = new MockPalomaClient
        {
            Sessions = [Session("a"), Session("b"), Session("c")],
        };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();
        vm.Move(1);
        vm.Move(1);
        vm.PendingDelete();

        Assert.True(await vm.ConfirmPendingDeleteAsync());

        Assert.Equal(2, vm.Rows.Count);
        Assert.Equal("b", vm.Selected!.Item.SessionId);
    }

    [Fact]
    public async Task CancelDelete_OnlyConsumesAPendingDeletion()
    {
        var mock = new MockPalomaClient { Sessions = [Session("a"), Session("b")] };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();

        // The bool routes the shell's Escape: true swallows the keypress,
        // false lets it fall through to leaving sessions mode.
        Assert.False(vm.CancelPendingDelete());

        vm.PendingDelete();
        Assert.True(vm.Selected!.PendingDeletion);

        Assert.True(vm.CancelPendingDelete());
        Assert.False(vm.Selected!.PendingDeletion);
        Assert.False(vm.CancelPendingDelete());
    }

    [Fact]
    public async Task PendingDelete_OnARow_SelectsAndMarksItPending()
    {
        var mock = new MockPalomaClient
        {
            Sessions = [Session("a"), Session("b"), Session("c")],
        };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();

        // The trash marks whichever row was clicked, hovered or not.
        vm.PendingDelete(vm.Rows[2]);
        Assert.Equal("c", vm.Selected!.Item.SessionId);
        Assert.True(vm.Rows[2].PendingDeletion);

        // Marking another row moves the confirmation with the selection.
        vm.PendingDelete(vm.Rows[1]);
        Assert.False(vm.Rows[2].PendingDeletion);
        Assert.Equal("b", vm.Selected!.Item.SessionId);

        // The second gesture — Enter or click — confirms the pending row.
        Assert.True(await vm.ConfirmPendingDeleteAsync());
        Assert.Equal(RemainingSessions, vm.Rows.Select(row => row.Item.SessionId));
    }

    [Fact]
    public async Task Move_ClearsThePendingDeletionOnTheRowLeftBehind()
    {
        var mock = new MockPalomaClient { Sessions = [Session("a"), Session("b")] };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();
        vm.PendingDelete();
        var pending = vm.Selected!;

        Assert.Equal(1, vm.Move(1));

        // Arrowing away must clear the pending deletion, so Enter on the
        // new row restores it instead of deleting the row left behind.
        Assert.False(pending.PendingDeletion);
        Assert.False(await vm.ConfirmPendingDeleteAsync());
        Assert.Equal(2, vm.Rows.Count);
    }

    [Fact]
    public async Task Search_ZeroMatches_DistinguishesFromEmptyStore()
    {
        var mock = new MockPalomaClient
        {
            Sessions = [Session("a"), Session("b")],
            OnSearchSessions = _ => [],
        };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();
        await vm.SearchAsync("zzz");

        Assert.Empty(vm.Rows);
        // Sessions exist; only the filter came up empty.
        Assert.Equal("No sessions match the search.", vm.Status);
    }

    [Fact]
    public async Task Search_RapidRetype_CancelsTheInFlightRpc()
    {
        var mock = new MockPalomaClient
        {
            Sessions = [Session("a"), Session("b")],
        };
        mock.OnSearchSessionsAsync = async (needle, token) =>
        {
            if (needle == "ne")
            {
                await Task.Delay(Timeout.InfiniteTimeSpan, token);
            }

            return ["b"];
        };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();

        var first = vm.SearchAsync("ne");
        var second = vm.SearchAsync("needle");
        await Task.WhenAll(first, second);

        // Debounce moved to the view; the model's guarantee is that the
        // retype kills the hung rpc and only the newest filter lands.
        Assert.Equal(RetypedNeedles, mock.SessionSearchCalls);
        Assert.Equal("b", Assert.Single(vm.Rows).Item.SessionId);
        Assert.Equal(string.Empty, vm.Status);
    }

    [Fact]
    public async Task CancelPendingDelete_ClearsThePendingStateAndKeepsTheSession()
    {
        var mock = new MockPalomaClient { Sessions = [Session("a"), Session("b")] };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();
        vm.PendingDelete(vm.Rows[0]);
        Assert.True(vm.Rows[0].PendingDeletion);

        // Hiding the window cancels the pending delete, so the next Enter
        // opens the session instead of deleting it.
        Assert.True(vm.CancelPendingDelete());

        Assert.False(vm.Rows[0].PendingDeletion);
        Assert.False(await vm.ConfirmPendingDeleteAsync());
        Assert.Equal(2, vm.Rows.Count);
    }

    [Fact]
    public async Task Search_ClearWhileAFilterHangs_StaysCleared()
    {
        var mock = new MockPalomaClient { Sessions = [Session("a"), Session("b")] };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();
        var gate = new TaskCompletionSource<IReadOnlyList<string>>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        mock.OnSearchSessionsAsync = (_, _) => gate.Task;

        var filter = vm.SearchAsync("ne");

        // The clear runs to completion synchronously and restores the
        // full list.
        var clear = vm.SearchAsync("");
        Assert.True(clear.IsCompleted);
        Assert.Equal(2, vm.Rows.Count);

        // The superseded filter answers late; it must not narrow the rows.
        gate.SetResult(["b"]);
        await filter.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(2, vm.Rows.Count);
    }

    [Fact]
    public async Task Search_ResponseAfterSupersession_IsDiscarded()
    {
        var mock = new MockPalomaClient
        {
            Sessions = [Session("a"), Session("b")],
        };
        var firstRpc = new TaskCompletionSource<IReadOnlyList<string>>();
        var secondRpc = new TaskCompletionSource<IReadOnlyList<string>>();
        var secondStarted = new TaskCompletionSource();
        mock.OnSearchSessionsAsync = (needle, _) =>
        {
            if (needle == "aa")
            {
                return firstRpc.Task;
            }

            secondStarted.TrySetResult();
            return secondRpc.Task;
        };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();

        var first = vm.SearchAsync("aa");
        var second = vm.SearchAsync("ab");
        await secondStarted.Task;

        // The first rpc succeeds only after the retype has fully claimed
        // the search state: a completed call throws nothing, so only the
        // first generation's own token check can drop its stale response.
        firstRpc.SetResult(["a"]);
        await first;
        Assert.Equal(2, vm.Rows.Count);

        secondRpc.SetResult(["b"]);
        await second;
        Assert.Equal("b", Assert.Single(vm.Rows).Item.SessionId);
    }

    [Fact]
    public async Task Search_WhenSuperseded_DoesNotReportFailure()
    {
        var mock = new MockPalomaClient { Sessions = [Session("a")] };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();
        var started = new TaskCompletionSource();
        mock.OnSearchSessionsAsync = async (_, token) =>
        {
            started.TrySetResult();
            await Task.Delay(Timeout.InfiniteTimeSpan, token);
            return [];
        };

        var first = vm.SearchAsync("needle");
        await started.Task;
        await vm.SearchAsync(string.Empty);
        await first;

        // A search the user already superseded must fail silently.
        Assert.Equal(string.Empty, vm.Status);
    }

    [Fact]
    public async Task Delete_WhenRpcFails_KeepsRowsAndReportsStatus()
    {
        var mock = new MockPalomaClient
        {
            Sessions = [Session("a"), Session("b"), Session("c")],
            OnRemoveSession = _ =>
                throw new InvalidOperationException("storage failure"),
        };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();

        await vm.RemoveAsync(vm.Rows[0]);

        Assert.Equal(3, vm.Rows.Count);
        Assert.Contains("storage failure", vm.Status);
    }

    [Fact]
    public async Task ConfirmDelete_WhenRpcFails_KeepsTheRowPendingForRetry()
    {
        var mock = new MockPalomaClient
        {
            Sessions = [Session("a"), Session("b")],
            OnRemoveSession = _ =>
                throw new InvalidOperationException("storage failure"),
        };
        var vm = new SessionsViewModel(mock);
        await vm.LoadAsync();
        vm.PendingDelete(vm.Rows[1]);

        Assert.True(await vm.ConfirmPendingDeleteAsync());

        // A failed rpc must leave the confirmation in place: the next
        // Enter or click retries the delete instead of starting over.
        Assert.Equal(2, vm.Rows.Count);
        Assert.Contains("storage failure", vm.Status);
        Assert.True(vm.Selected!.PendingDeletion);
        Assert.Equal("b", vm.Selected.Item.SessionId);
    }
}