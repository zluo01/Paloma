using CommunityToolkit.Mvvm.Messaging;
using Paloma.Messages;
using Paloma.ViewModels.Overlay;
using Xunit;
using Connector = PalomaCore.Connector;
using ConnectorConnection = PalomaCore.ConnectorConnection;
using HealthLevel = PalomaCore.HealthLevel;
using HealthStatus = PalomaCore.HealthStatus;
using Model = PalomaCore.Model;
using ProviderBackendId = PalomaCore.ProviderBackendId;
using ProviderStatus = PalomaCore.ProviderStatus;

namespace Paloma.Tests;

using static TestProtos;

public sealed class FooterViewModelTests
{
    [Fact]
    public async Task Refresh_WithResolvablePreference_SelectsIt()
    {
        var mock = new MockPalomaClient
        {
            Connectors =
            [
                ConnectorWith("other"),
                ConnectorWith("preferred", preferred: true, preferModel: "b", preferEffort: "high"),
            ],
        };
        var (vm, errors) = Footer(mock);

        await vm.RefreshAsync();

        Assert.Equal("b", vm.SelectedModelId);
        Assert.Equal("high", vm.SelectedEffort);
        Assert.Equal("Model B · high", vm.ModelLabel);
        Assert.True(vm.HasSelectableProvider);
        Assert.Equal(HealthLevel.Healthy, vm.ServicesHealth);
        Assert.Equal(HealthLevel.Healthy, vm.PluginsHealth);
        Assert.Empty(errors);
    }

    [Fact]
    public async Task Refresh_WithoutStoredPreference_AsksToSelectInsteadOfGuessing()
    {
        var mock = new MockPalomaClient { Connectors = [ConnectorWith("backend")] };
        var (vm, _) = Footer(mock);

        await vm.RefreshAsync();

        // No fallback to the first model: an unset preference stays unset.
        Assert.Equal(string.Empty, vm.SelectedModelId);
        Assert.Equal("Select model", vm.ModelLabel);
        Assert.True(vm.HasSelectableProvider);
    }

    [Fact]
    public async Task Refresh_WhenThePreferredModelWasDropped_AsksToSelect()
    {
        var mock = new MockPalomaClient
        {
            Connectors =
                [ConnectorWith("backend", preferred: true, preferModel: "gone", preferEffort: "high")],
        };
        var (vm, _) = Footer(mock);

        await vm.RefreshAsync();

        Assert.Equal("Select model", vm.ModelLabel);
        Assert.Equal(string.Empty, vm.SelectedModelId);
    }

    [Fact]
    public async Task Refresh_WhenThePreferredEffortWasRetired_AsksToSelect()
    {
        var mock = new MockPalomaClient
        {
            Connectors =
                [ConnectorWith("backend", preferred: true, preferModel: "b", preferEffort: "retired")],
        };
        var (vm, _) = Footer(mock);

        await vm.RefreshAsync();

        Assert.Equal("Select model", vm.ModelLabel);
        Assert.Equal(string.Empty, vm.SelectedModelId);
    }

    [Fact]
    public async Task Refresh_WhenAllProvidersAreUnhealthy_DisablesThePicker()
    {
        var mock = new MockPalomaClient
        {
            Connectors =
            [
                ConnectorWith("one", health: HealthStatus.Unhealthy),
                ConnectorWith(
                    "two",
                    preferred: true,
                    preferModel: "b",
                    preferEffort: "high",
                    health: HealthStatus.Unhealthy),
            ],
        };
        var (vm, _) = Footer(mock);

        await vm.RefreshAsync();

        Assert.False(vm.HasSelectableProvider);
        Assert.Equal("No model", vm.ModelLabel);
        // Unhealthy connections stay listed so the menu can show them as
        // greyed-out rows.
        Assert.Equal(2, vm.Connected.Count);
    }

    [Fact]
    public async Task Refresh_WhenThePreferredProviderIsUnhealthy_IgnoresItsPreference()
    {
        var mock = new MockPalomaClient
        {
            Connectors =
            [
                ConnectorWith(
                    "down",
                    preferred: true,
                    preferModel: "b",
                    preferEffort: "high",
                    health: HealthStatus.Starting),
                ConnectorWith("up"),
            ],
        };
        var (vm, _) = Footer(mock);

        await vm.RefreshAsync();

        Assert.Equal("Select model", vm.ModelLabel);
        Assert.Equal(string.Empty, vm.SelectedModelId);
        Assert.True(vm.HasSelectableProvider);
    }

    [Fact]
    public async Task Refresh_WhenModelsOfferNoEfforts_DisablesThePicker()
    {
        var mock = new MockPalomaClient
        {
            Connectors = [ConnectorWith("backend", models: [TestModel("bare", "Bare", "")])],
        };
        var (vm, _) = Footer(mock);

        await vm.RefreshAsync();

        // A model with no efforts could only persist a preference the next
        // refresh cannot resolve.
        Assert.False(vm.HasSelectableProvider);
        Assert.Equal("No model", vm.ModelLabel);
    }

    [Fact]
    public async Task Refresh_WithNoConnectedProviders_ShowsNoModel()
    {
        var mock = new MockPalomaClient { Connectors = [Unconnected("backend")] };
        var (vm, _) = Footer(mock);

        await vm.RefreshAsync();

        Assert.Empty(vm.Connected);
        Assert.False(vm.HasSelectableProvider);
        Assert.Equal("No model", vm.ModelLabel);
    }

    [Fact]
    public async Task Refresh_WhenRpcFails_ReportsAndClears()
    {
        var mock = new MockPalomaClient
        {
            Connectors =
                [ConnectorWith("backend", preferred: true, preferModel: "b", preferEffort: "high")],
        };
        var (vm, errors) = Footer(mock);
        await vm.RefreshAsync();
        Assert.Equal("b", vm.SelectedModelId);

        mock.OnGetConnectors = () =>
            throw new InvalidOperationException("core is down");
        await vm.RefreshAsync();

        Assert.Contains("core is down", Assert.Single(errors));
        Assert.Equal(HealthLevel.Down, vm.ServicesHealth);
        Assert.Equal(HealthLevel.Down, vm.PluginsHealth);
        Assert.Empty(vm.Connected);
        Assert.False(vm.HasSelectableProvider);
        Assert.Equal("No model", vm.ModelLabel);
        Assert.Equal(string.Empty, vm.SelectedModelId);
    }

    [Fact]
    public async Task SelectModel_UpdatesTheLabelAndPersists()
    {
        var mock = new MockPalomaClient();
        var (vm, errors) = Footer(mock);

        await vm.SelectModelAsync(Backend("backend"), ModelB, "high");

        Assert.Equal("b", vm.SelectedModelId);
        Assert.Equal("high", vm.SelectedEffort);
        Assert.Equal("Model B · high", vm.ModelLabel);
        var call = Assert.Single(mock.ModelPreferences);
        Assert.Equal(("b", "high"), (call.Model, call.Effort));
        Assert.Empty(errors);
    }

    [Fact]
    public async Task SelectModel_WithASingleEffortModel_LabelsWithoutTheEffort()
    {
        var mock = new MockPalomaClient();
        var (vm, _) = Footer(mock);

        await vm.SelectModelAsync(Backend("backend"), TestModel("s", "Solo", "only", "only"), "only");

        Assert.Equal("Solo", vm.ModelLabel);
    }

    [Fact]
    public async Task SelectModel_WhenPersistFails_Reports()
    {
        var mock = new MockPalomaClient
        {
            OnSetModelPreference = (_, _, _) =>
                throw new InvalidOperationException("core is down"),
        };
        var (vm, errors) = Footer(mock);

        await vm.SelectModelAsync(Backend("backend"), ModelB, "high");

        // The label keeps the choice; the failure surfaces on the banner.
        Assert.Equal("Model B · high", vm.ModelLabel);
        Assert.Contains("core is down", Assert.Single(errors));
    }

    private static (FooterViewModel Vm, List<string> Errors) Footer(MockPalomaClient mock)
    {
        var (messenger, errors) = TestMessenger.WithErrorSink();
        return (new FooterViewModel(mock, messenger), errors);
    }
}