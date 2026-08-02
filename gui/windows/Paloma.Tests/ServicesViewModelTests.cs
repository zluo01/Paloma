using Grpc.Core;
using Paloma.ViewModels.Settings;
using Xunit;
using Connector = Paloma.Binding.V1.Connector;
using ConnectorConnection = Paloma.Binding.V1.ConnectorConnection;
using Model = Paloma.Provider.Runtime.V1.Model;
using ProviderBackendId = Paloma.Binding.V1.ProviderBackendId;
using ProviderStatus = Paloma.Binding.V1.ProviderStatus;

namespace Paloma.Tests;

using static TestProtos;

public sealed class ServicesViewModelTests
{
    [Fact]
    public async Task Disconnect_WhenRpcFails_ReportsInsteadOfCrashing()
    {
        var connector = Preferring(
            "a", "medium", models: [TestModel("a", "Model A", "medium", "medium")]);
        var mock = new MockPalomaClient
        {
            OnDisconnect = _ =>
                throw new RpcException(new Status(StatusCode.Unavailable, "core is down")),
        };
        var reported = string.Empty;
        var vm = new ConnectorViewModel(
            mock, connector, () => Task.CompletedTask, message => reported = message);

        // The command surface is what the button invokes; a faulted task
        // here is rethrown onto the UI thread and kills the app.
        await vm.DisconnectCommand.ExecuteAsync(null);

        Assert.Contains("core is down", reported);
    }

    [Fact]
    public async Task ModelSwitch_WhenPersistFails_ReportsInsteadOfShowingSaved()
    {
        var modelA = TestModel("a", "Model A", "medium", "low", "medium");
        var modelB = TestModel("b", "Model B", "medium", "low", "medium");
        var connector = Preferring("a", "medium", models: [modelA, modelB]);
        var mock = new MockPalomaClient
        {
            OnSetModelPreference = (_, _, _) =>
                throw new RpcException(new Status(StatusCode.Unavailable, "core is down")),
        };
        var reported = string.Empty;
        var vm = new ConnectorViewModel(
            mock, connector, () => Task.CompletedTask, message => reported = message);

        vm.SelectedModel = modelB;

        // The pickers keep showing a choice the core never stored; the
        // failure must at least surface in the error banner.
        await TestWait.UntilAsync(() => reported.Length > 0);
        Assert.Contains("core is down", reported);
    }

    [Fact]
    public async Task ModelSwitch_WithMatchingDefaultEffort_PersistsPreference()
    {
        var modelA = TestModel("a", "Model A", "medium", "low", "medium");
        var modelB = TestModel("b", "Model B", "medium", "low", "medium");
        var connector = Preferring("a", "medium", models: [modelA, modelB]);
        var mock = new MockPalomaClient();
        var vm = new ConnectorViewModel(mock, connector, () => Task.CompletedTask, _ => { });

        vm.SelectedModel = modelB;

        // Switching models must persist even when the new model's default
        // effort equals the currently selected effort string.
        await TestWait.UntilAsync(
            () => mock.ModelPreferences.Any(preference => preference.Model == "b"));
    }

    [Fact]
    public async Task Load_PartitionsConnectorsByConnection()
    {
        var mock = new MockPalomaClient
        {
            Connectors = [ConnectedConnector("b1"), AvailableConnector("a1")],
        };
        var vm = new ServicesViewModel(mock);

        await vm.LoadAsync();

        Assert.Single(vm.Connected);
        Assert.Single(vm.Available);
        Assert.True(vm.Ready);
        Assert.False(vm.Loading);
        Assert.False(vm.HasError);
        Assert.False(vm.ShowNoConnected);
        Assert.False(vm.ShowNoAvailable);
    }

    [Fact]
    public async Task Load_WithNoConnectors_ShowsBothEmptyStates()
    {
        var vm = new ServicesViewModel(new MockPalomaClient());

        await vm.LoadAsync();

        Assert.True(vm.Ready);
        Assert.True(vm.ShowNoConnected);
        Assert.True(vm.ShowNoAvailable);
    }

    [Fact]
    public async Task Load_WhenRefreshFails_ReportsAndHidesEmptyStates()
    {
        var mock = new MockPalomaClient
        {
            OnGetConnectors = () =>
                throw new RpcException(new Status(StatusCode.Unavailable, "core is down")),
        };
        var vm = new ServicesViewModel(mock);

        await vm.LoadAsync();

        Assert.False(vm.Ready);
        Assert.False(vm.Loading);
        Assert.Contains("core is down", vm.Error);
        // A failed load is unknown state: the page must not claim emptiness.
        Assert.False(vm.ShowNoConnected);
        Assert.False(vm.ShowNoAvailable);
    }

    [Fact]
    public async Task Refresh_AfterFailure_ClearsTheError()
    {
        var mock = new MockPalomaClient
        {
            OnGetConnectors = () =>
                throw new RpcException(new Status(StatusCode.Unavailable, "core is down")),
        };
        var vm = new ServicesViewModel(mock);
        await vm.LoadAsync();
        Assert.True(vm.HasError);

        mock.OnGetConnectors = null;
        mock.Connectors = [AvailableConnector("a1")];

        Assert.True(await vm.RefreshAsync());
        Assert.False(vm.HasError);
        Assert.Single(vm.Available);
    }

    [Fact]
    public async Task Refresh_RebuildsRowsFromTheCurrentAnswer()
    {
        var mock = new MockPalomaClient
        {
            Connectors = [ConnectedConnector("b1"), ConnectedConnector("b2")],
        };
        var vm = new ServicesViewModel(mock);
        await vm.LoadAsync();
        Assert.Equal(2, vm.Connected.Count);

        mock.Connectors = [ConnectedConnector("b1"), AvailableConnector("a1")];
        await vm.RefreshAsync();

        Assert.Single(vm.Connected);
        Assert.Single(vm.Available);
    }

    [Fact]
    public async Task RowPersistFailure_LandsOnThePageError()
    {
        var mock = new MockPalomaClient
        {
            Connectors = [ConnectedConnector("b1")],
            OnSetModelPreference = (_, _, _) =>
                throw new RpcException(new Status(StatusCode.Unavailable, "core is down")),
        };
        var vm = new ServicesViewModel(mock);
        await vm.LoadAsync();

        vm.Connected[0].SelectedEffort = "low";

        await TestWait.UntilAsync(() => vm.HasError);
        Assert.Contains("core is down", vm.Error);
    }

    [Fact]
    public async Task RowDisconnect_RefreshesThePage()
    {
        var mock = new MockPalomaClient { Connectors = [ConnectedConnector("b1")] };
        var vm = new ServicesViewModel(mock);
        await vm.LoadAsync();

        mock.Connectors = [AvailableConnector("a1")];
        await vm.Connected[0].DisconnectCommand.ExecuteAsync(null);

        Assert.Empty(vm.Connected);
        Assert.Single(vm.Available);
    }

    [Fact]
    public async Task Seeding_NeverPersists()
    {
        var mock = new MockPalomaClient();
        _ = Row(mock, Preferring("b", "high"));

        // A stray async-void persist gets a beat to surface before asserting.
        await Task.Delay(50);
        Assert.Empty(mock.ModelPreferences);
    }

    [Fact]
    public void Seeding_PicksTheStoredPreference()
    {
        var vm = Row(new MockPalomaClient(), Preferring("b", "medium"));

        Assert.Equal("b", vm.SelectedModel?.Id);
        Assert.Equal("medium", vm.SelectedEffort);
        Assert.Equal(ModelB.SupportedReasoningEfforts, vm.Efforts);
    }

    [Fact]
    public void Seeding_FallsBackToFirstModelAndItsDefaultEffort()
    {
        var vm = Row(new MockPalomaClient(), Preferring("missing", "weird"));

        Assert.Equal("a", vm.SelectedModel?.Id);
        Assert.Equal("medium", vm.SelectedEffort);
    }

    [Fact]
    public void ShowPickers_RequiresHealthyConnectionWithModels()
    {
        var mock = new MockPalomaClient();
        Assert.True(Row(mock, Preferring("a", "medium")).ShowPickers);
        Assert.False(Row(mock, Preferring("a", "medium", error: "token expired")).ShowPickers);
        Assert.False(Row(mock, Preferring("a", "medium", models: [])).ShowPickers);
    }

    [Fact]
    public void StatuslessConnection_ShowsNoPickersAndNoModels()
    {
        // The proto allows a connection without a status message; the row
        // must come up empty instead of crashing.
        var connector = new Connector
        {
            Id = new ProviderBackendId { ProviderId = "provider", BackendId = "backend" },
            Description = "a test connector",
            Connection = new ConnectorConnection
            {
                Preferred = true,
                PreferModel = "a",
                PreferEffort = "medium",
            },
        };

        var vm = Row(new MockPalomaClient(), connector);

        Assert.Empty(vm.Models);
        Assert.False(vm.ShowPickers);
        Assert.Null(vm.SelectedModel);
        Assert.Null(vm.SelectedEffort);
    }

    [Fact]
    public void Error_SurfacesTheStatusErrorOrNull()
    {
        var mock = new MockPalomaClient();

        Assert.Equal(
            "token expired",
            Row(mock, Preferring("a", "medium", error: "token expired")).Error);
        Assert.Null(Row(mock, Preferring("a", "medium")).Error);
    }

    [Fact]
    public async Task ModelSwitch_PersistsExactlyOnce()
    {
        var mock = new MockPalomaClient();
        var vm = Row(mock, Preferring("a", "medium"));

        vm.SelectedModel = ModelB;

        await TestWait.UntilAsync(() => mock.ModelPreferences.Count > 0);
        // A cascade double-persist would land in this window.
        await Task.Delay(50);
        var call = Assert.Single(mock.ModelPreferences);
        Assert.Equal("b", call.Model);
        Assert.Equal("high", call.Effort);
    }

    [Fact]
    public async Task EffortChange_PersistsTheChoice()
    {
        var mock = new MockPalomaClient();
        var vm = Row(mock, Preferring("a", "medium"));

        vm.SelectedEffort = "low";

        await TestWait.UntilAsync(() => mock.ModelPreferences.Count > 0);
        var call = Assert.Single(mock.ModelPreferences);
        Assert.Equal("a", call.Model);
        Assert.Equal("low", call.Effort);
    }

    private static ConnectorViewModel Row(MockPalomaClient mock, Connector connector) =>
        new(mock, connector, () => Task.CompletedTask, _ => { });

    private static Connector Preferring(
        string preferModel,
        string preferEffort,
        string? error = null,
        IReadOnlyList<Model>? models = null) =>
        ConnectorWith(
            preferred: true,
            preferModel: preferModel,
            preferEffort: preferEffort,
            error: error,
            models: models);

    private static Connector ConnectedConnector(string backend) =>
        ConnectorWith(backend, preferred: true, preferModel: "a", preferEffort: "medium", models: [ModelA]);

    private static Connector AvailableConnector(string backend) => Unconnected(backend);
}
