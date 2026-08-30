using CommunityToolkit.Mvvm.Messaging;
using Paloma.Helpers;
using Paloma.Messages;
using Paloma.ViewModels.Overlay;
using Xunit;
using CapabilityIcon = PalomaCore.Icon;
using ExtAction = PalomaCore.Action;
using ExtensionCapabilityId = PalomaCore.ExtensionCapabilityId;
using Item = PalomaCore.Item;
using QueryResponse = PalomaCore.QueryResponse;
using Behavior = PalomaCore.Behavior;

namespace Paloma.Tests;

public sealed class SearchViewModelTests
{
    private static readonly string[] RetypedQueries = ["re", "readme"];

    private static QueryResponse Section(string name, params string[] items)
    {
        return new QueryResponse(
            new ExtensionCapabilityId("ext", "cap"),
            name,
            [.. items.Select(title => new Item(title, null, null, [new ExtAction("Open", [], true)]))]);
    }

    private static (SearchViewModel Vm, List<string> Errors) Model(MockPalomaClient mock)
    {
        var (messenger, errors) = TestMessenger.WithErrorSink();
        return (new SearchViewModel(mock, messenger), errors);
    }

    [Fact]
    public async Task Activate_WhileTheActionRuns_IgnoresRepeatTriggers()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        var gate = new TaskCompletionSource<Behavior>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        var calls = 0;
        mock.OnRunActionAsync = (_, _) =>
        {
            calls++;
            return gate.Task;
        };
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(Section("Apps", "app one"));
        await vm.SearchAsync("query");
        var row = vm.SelectedRow!;
        var action = row.Item.Actions[0];

        var first = vm.ActivateAsync(row, action);

        // A held Enter or a double click re-triggers while the RPC runs;
        // only the first activation may reach the client.
        Assert.Null(await vm.ActivateAsync(row, action));
        Assert.Equal(1, calls);

        gate.SetResult(new Behavior.Stay());
        Assert.NotNull(await first);
    }

    [Fact]
    public async Task Search_ClearThenRetype_KeepsTheNewQueryResults()
    {
        var mock = new MockPalomaClient();
        var (vm, errors) = Model(mock);
        var started = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);
        var gate = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);
        mock.OnSearch = (query, _) => query == "aa"
            ? HangThenPublish()
            : MockPalomaClient.Stream(Section("Files", "from cc"));

        async IAsyncEnumerable<QueryResponse> HangThenPublish()
        {
            started.SetResult();
            // Ignores the token: the superseded run survives the cancel
            // and tries to publish a stale section later.
            await gate.Task;
            yield return Section("Stale", "stale row");
        }

        var searchA = vm.SearchAsync("aa");
        await started.Task.WaitAsync(TimeSpan.FromSeconds(5));

        // The clear runs to completion synchronously: no await sits
        // between the cancel and the reset.
        var clear = vm.SearchAsync("");
        Assert.True(clear.IsCompleted);
        Assert.Empty(vm.Groups);

        await vm.SearchAsync("cc").WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal("Files", Assert.Single(vm.Groups).Name);

        gate.SetResult();
        await searchA.WaitAsync(TimeSpan.FromSeconds(5));

        // The superseded run's late section must not land.
        Assert.Equal("Files", Assert.Single(vm.Groups).Name);
        Assert.Empty(errors);
    }

    [Fact]
    public async Task AskSelection_SurvivesLateArrivingSection()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        var gate = new TaskCompletionSource();
        mock.OnSearch = (_, _) => TwoSections();

        async IAsyncEnumerable<QueryResponse> TwoSections()
        {
            yield return Section("Apps", "app one");
            await gate.Task;
            yield return Section("Files", "file one");
        }

        var search = vm.SearchAsync("query");
        await TestWait.UntilAsync(() => vm.Groups.Count == 1);
        vm.Move(1);
        Assert.True(vm.AskSelected);
        Assert.Null(vm.SelectedRow);

        gate.SetResult();
        await search;

        // A slower section arriving must not steal the selection off the
        // ask row the user had already arrowed onto.
        Assert.True(vm.AskSelected);
        Assert.Null(vm.SelectedRow);
    }

    [Fact]
    public async Task Move_PastTheLastRow_SelectsTheAskRow()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(Section("Apps", "app one"));
        await vm.SearchAsync("query");
        Assert.Equal("app one", vm.SelectedRow?.Item.Title);

        // The ask row has no flat index to report.
        Assert.Equal(-1, vm.Move(1));
        Assert.True(vm.AskSelected);
        Assert.Null(vm.SelectedRow);

        // Down from the ask row has nowhere to go.
        Assert.Equal(-1, vm.Move(1));
        Assert.True(vm.AskSelected);

        Assert.Equal(0, vm.Move(-1));
        Assert.False(vm.AskSelected);
        Assert.Equal("app one", vm.SelectedRow?.Item.Title);
    }

    [Fact]
    public async Task Move_WalksAcrossGroups()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(
            Section("Apps", "a1", "a2"),
            Section("Files", "f1"));
        await vm.SearchAsync("query");
        Assert.Equal("a1", vm.SelectedRow?.Item.Title);

        vm.Move(1);
        // The flat selection crosses the group boundary.
        Assert.Equal(2, vm.Move(1));
        Assert.Equal("f1", vm.SelectedRow?.Item.Title);

        Assert.Equal(1, vm.Move(-1));
        Assert.Equal("a2", vm.SelectedRow?.Item.Title);
    }

    [Fact]
    public async Task Select_OnARow_MovesTheSelectionThere()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(
            Section("Apps", "a1", "a2"),
            Section("Files", "f1"));
        await vm.SearchAsync("query");
        var target = vm.Groups[1].Items[0];

        vm.Select(target);

        Assert.Same(target, vm.SelectedRow);
        Assert.False(vm.Groups[0].Items[0].IsSelected);

        // A row that is not in the list moves nothing.
        vm.Select(LauncherRow.ForItem(
            new ExtensionCapabilityId("ext", "cap"),
            new Item("foreign", null, null, [new ExtAction("Open", [], false)])));
        Assert.Same(target, vm.SelectedRow);
    }

    [Fact]
    public async Task Select_OnARow_ClearsTheAskSelection()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(Section("Apps", "app one"));
        await vm.SearchAsync("query");
        vm.Move(1);
        Assert.True(vm.AskSelected);

        vm.Select(vm.Groups[0].Items[0]);

        Assert.False(vm.AskSelected);
        Assert.Equal("app one", vm.SelectedRow?.Item.Title);
    }

    [Fact]
    public async Task Move_UpAtTheFirstRow_StaysPut()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(Section("Apps", "app one"));
        await vm.SearchAsync("query");

        // A clamped move still reports the row that stays selected.
        Assert.Equal(0, vm.Move(-1));

        Assert.Equal("app one", vm.SelectedRow?.Item.Title);
    }

    [Fact]
    public void Move_WithNoResults_DoesNothing()
    {
        var (vm, _) = Model(new MockPalomaClient());

        Assert.Equal(-1, vm.Move(1));
        Assert.Equal(-1, vm.Move(-1));

        Assert.Null(vm.SelectedRow);
        Assert.False(vm.AskSelected);
    }

    [Fact]
    public async Task Search_NewQuery_ResetsSelectionToTheFirstRow()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (query, _) => query == "first"
            ? MockPalomaClient.Stream(Section("Apps", "one", "two", "three"))
            : MockPalomaClient.Stream(Section("Files", "readme"));
        await vm.SearchAsync("first");
        vm.Move(1);
        vm.Move(1);
        Assert.Equal("three", vm.SelectedRow?.Item.Title);

        await vm.SearchAsync("second");

        // A new query's results start over from the first row.
        Assert.Equal("readme", vm.SelectedRow?.Item.Title);
        Assert.Equal("second", vm.Query);
    }

    [Fact]
    public async Task Search_NewQuery_ClearsTheAskSelection()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(Section("Apps", "app one"));
        await vm.SearchAsync("first");
        vm.Move(1);
        Assert.True(vm.AskSelected);

        await vm.SearchAsync("second");

        Assert.False(vm.AskSelected);
        Assert.Equal("app one", vm.SelectedRow?.Item.Title);
    }

    [Fact]
    public async Task Search_DropsItemsWithoutActions()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        var section = Section("Apps", "usable");
        section = section with { Items = [.. section.Items, new Item("inert", null, null, [])] };
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(section);

        await vm.SearchAsync("query");

        // Rows without actions cannot be activated, so they never land.
        var group = Assert.Single(vm.Groups);
        Assert.Equal("usable", Assert.Single(group.Items).Item.Title);
    }

    [Fact]
    public async Task Search_WhenEveryItemLacksActions_ClearsThePreviousRows()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(Section("Apps", "app one"));
        await vm.SearchAsync("first");
        Assert.True(vm.HasResults);

        var inert = Section("Files");
        inert = inert with { Items = [new Item("no actions", null, null, [])] };
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(inert);
        await vm.SearchAsync("second");

        // A section whose rows all filter out is not a result; it must land
        // exactly like a sectionless stream.
        Assert.Empty(vm.Groups);
        Assert.False(vm.HasResults);
    }

    [Fact]
    public void ForItem_WithANameIcon_LoadsNoImage()
    {
        var row = LauncherRow.ForItem(
            new ExtensionCapabilityId("ext", "cap"),
            new Item("title", null, new CapabilityIcon.Name("image-png"), [new ExtAction("Open", [], true)]));

        // Windows has no themed-icon registry to resolve a name against;
        // the row collapses its icon slot instead.
        Assert.Null(row.Icon);
        Assert.False(row.ShowsIcon);
    }

    [Fact]
    public void RowHints_YieldToTheHoveringMouse()
    {
        var row = LauncherRow.ForItem(
            new ExtensionCapabilityId("ext", "cap"),
            new Item("title", null, null, [new ExtAction("Open", [], true), new ExtAction("Copy", [], false)]));

        row.IsSelected = true;
        Assert.True(row.ShowActionHint);
        Assert.False(row.ShowMoreButton);

        // The mouse on the row swaps the keyboard chip for the more button.
        row.IsHovered = true;
        Assert.False(row.ShowActionHint);
        Assert.True(row.ShowMoreButton);

        // A single-action row offers neither affordance.
        var single = LauncherRow.ForItem(
            new ExtensionCapabilityId("ext", "cap"),
            new Item("one", null, null, [new ExtAction("Open", [], true)]));
        single.IsSelected = true;
        single.IsHovered = true;
        Assert.False(single.ShowActionHint);
        Assert.False(single.ShowMoreButton);
    }

    [Fact]
    public void ForItem_WithAPathIcon_ReservesTheIconSlot()
    {
        var row = LauncherRow.ForItem(
            new ExtensionCapabilityId("ext", "cap"),
            new Item("title", null, new CapabilityIcon.Path(@"Z:\paloma-tests\missing-slot"),
                [new ExtAction("Open", [], true)]));

        // The slot is decided up front so the row never shifts when the
        // async render lands.
        Assert.True(row.ShowsIcon);

        // At first render the image is still pending, so a slot keyed on
        // the loaded icon would collapse here and pop open later.
        Assert.Null(row.Icon);
    }

    [Fact]
    public async Task Search_KeepsPreviousRowsUntilTheFirstNewSectionArrives()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(Section("Apps", "app one"));
        await vm.SearchAsync("first");
        Assert.Equal("Apps", Assert.Single(vm.Groups).Name);

        var started = new TaskCompletionSource();
        var gate = new TaskCompletionSource();
        mock.OnSearch = (_, _) => Held();

        async IAsyncEnumerable<QueryResponse> Held()
        {
            started.SetResult();
            await gate.Task;
            yield return Section("Files", "file one");
        }

        var second = vm.SearchAsync("second");
        await started.Task;

        // The overlay resizes off Groups changes; clearing before the new
        // stream produces anything pumps the window empty and back on
        // every keystroke pause.
        Assert.Equal("Apps", Assert.Single(vm.Groups).Name);

        gate.SetResult();
        await second;
        Assert.Equal("Files", Assert.Single(vm.Groups).Name);
    }

    [Fact]
    public async Task Search_WithNoSections_ClearsThePreviousRows()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        mock.OnSearch = (_, _) => MockPalomaClient.Stream(Section("Apps", "app one"));
        await vm.SearchAsync("first");
        Assert.Single(vm.Groups);
        Assert.True(vm.HasResults);

        mock.OnSearch = (_, _) => MockPalomaClient.Stream<QueryResponse>();
        await vm.SearchAsync("second");

        // A resultless query shows no list at all; enter still starts a chat.
        Assert.Empty(vm.Groups);
        Assert.False(vm.HasResults);
    }

    [Fact]
    public async Task Clear_MidStream_StopsLateSectionsFromResurfacing()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Model(mock);
        var gate = new TaskCompletionSource();
        mock.OnSearch = (_, _) => TwoSections();

        async IAsyncEnumerable<QueryResponse> TwoSections()
        {
            yield return Section("Apps", "app one");
            await gate.Task;
            yield return Section("Files", "file one");
        }

        var search = vm.SearchAsync("query");
        await TestWait.UntilAsync(() => vm.Groups.Count > 0);

        // Leaving search mode empties the list; a still-running stream must
        // not repopulate it from a late section.
        vm.Clear();
        gate.SetResult();
        await search;

        Assert.Empty(vm.Groups);
    }

    [Fact]
    public async Task Search_WhenCoreUnavailable_ReportsError()
    {
        var mock = new MockPalomaClient();
        var (vm, errors) = Model(mock);
        mock.OnSearch = (_, _) => Failing();

        async IAsyncEnumerable<QueryResponse> Failing()
        {
            await Task.FromException(
                new InvalidOperationException("core is down"));
            yield break;
        }

        await vm.SearchAsync("query");

        Assert.Contains("core is down", Assert.Single(errors));
    }

    [Fact]
    public async Task Search_RapidRetype_CancelsTheInFlightStream()
    {
        var mock = new MockPalomaClient();
        var (vm, errors) = Model(mock);
        mock.OnSearch = (query, token) => query == "re"
            ? Held(token)
            : MockPalomaClient.Stream(Section("Files", "readme"));

        async IAsyncEnumerable<QueryResponse> Held(
            [System.Runtime.CompilerServices.EnumeratorCancellation]
            CancellationToken token)
        {
            await Task.Delay(Timeout.InfiniteTimeSpan, token);
            yield break;
        }

        var first = vm.SearchAsync("re");
        var second = vm.SearchAsync("readme");
        await Task.WhenAll(first, second);

        // Debounce moved to the view; the model's guarantee is that the
        // retype kills the hung stream and only the newest rows land.
        Assert.Equal(RetypedQueries, mock.SearchCalls);
        Assert.Equal("Files", Assert.Single(vm.Groups).Name);
        Assert.Empty(errors);
    }

    [Fact]
    public async Task Search_SectionAfterSupersession_IsDiscarded()
    {
        var mock = new MockPalomaClient();
        var (vm, errors) = Model(mock);
        var firstStarted = new TaskCompletionSource();
        var gate = new TaskCompletionSource();
        mock.OnSearch = (query, _) => query == "first"
            ? Held()
            : MockPalomaClient.Stream(Section("Files", "readme"));

        async IAsyncEnumerable<QueryResponse> Held()
        {
            firstStarted.SetResult();
            await gate.Task;
            yield return Section("Apps", "stale one");
        }

        var first = vm.SearchAsync("first");
        await firstStarted.Task;
        // The retype cancels and disposes the first search's source.
        await vm.SearchAsync("second");
        Assert.Equal("Files", Assert.Single(vm.Groups).Name);

        // A section the first stream still yields must be discarded via the
        // disposed source's safe reads instead of landing or crashing.
        gate.SetResult();
        await first;

        Assert.Equal("Files", Assert.Single(vm.Groups).Name);
        Assert.Empty(errors);
    }

    [Fact]
    public async Task Search_WhenSuperseded_DoesNotReportError()
    {
        var mock = new MockPalomaClient();
        var (vm, errors) = Model(mock);
        var started = new TaskCompletionSource();
        mock.OnSearch = (_, token) => Held(token);

        async IAsyncEnumerable<QueryResponse> Held(
            [System.Runtime.CompilerServices.EnumeratorCancellation]
            CancellationToken token)
        {
            started.TrySetResult();
            await Task.Delay(Timeout.InfiniteTimeSpan, token);
            yield break;
        }

        var first = vm.SearchAsync("query");
        await started.Task;
        await vm.SearchAsync(string.Empty);
        await first;

        // A search the user already superseded must fail silently.
        Assert.Empty(errors);
    }

    [Fact]
    public async Task Action_WhenRpcFails_ReportsErrorAndReturnsNull()
    {
        var mock = new MockPalomaClient
        {
            OnRunAction = (_, _) =>
                throw new InvalidOperationException("action broke"),
        };
        var (vm, errors) = Model(mock);
        var action = new ExtAction("Open", [], true);
        var row = LauncherRow.ForItem(
            new ExtensionCapabilityId("ext", "cap"),
            new Item("title", null, null, [action]));

        var behavior = await vm.ActivateAsync(row, action);

        Assert.Null(behavior);
        Assert.Contains("action broke", Assert.Single(errors));
    }
}