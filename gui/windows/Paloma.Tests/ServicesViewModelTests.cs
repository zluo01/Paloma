using Paloma.ViewModels.Settings;
using Xunit;
using Connector = PalomaCore.Connector;
using ConnectorConnection = PalomaCore.ConnectorConnection;
using ProviderBackendId = PalomaCore.ProviderBackendId;

namespace Paloma.Tests;

using static TestProtos;

public sealed class ServicesViewModelTests
{
    [Fact]
    public async Task Disconnect_WhenRpcFails_ReportsInsteadOfCrashing()
    {
        var mock = new MockPalomaClient
        {
            OnDisconnect = _ =>
                throw new InvalidOperationException("core is down"),
        };
        var reported = string.Empty;
        var vm = new ConnectorViewModel(
            mock, ConnectorWith(), () => Task.CompletedTask, message => reported = message);

        // The command surface is what the button invokes; a faulted task
        // here is rethrown onto the UI thread and kills the app.
        await vm.DisconnectCommand.ExecuteAsync(null);

        Assert.Contains("core is down", reported);
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
                throw new InvalidOperationException("core is down"),
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
                throw new InvalidOperationException("core is down"),
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
    public void Error_SurfacesTheStatusErrorOrNull()
    {
        var mock = new MockPalomaClient();
        var erroring = Row(mock, ConnectorWith(error: "token expired"));
        var healthy = Row(mock, ConnectorWith());

        Assert.Equal("token expired", erroring.Error);
        Assert.True(erroring.HasError);
        Assert.Null(healthy.Error);
        Assert.False(healthy.HasError);
    }

    private static ConnectorViewModel Row(MockPalomaClient mock, Connector connector) =>
        new(mock, connector, () => Task.CompletedTask, _ => { });

    private static Connector ConnectedConnector(string backend) => ConnectorWith(backend);

    private static Connector AvailableConnector(string backend) => Unconnected(backend);
}