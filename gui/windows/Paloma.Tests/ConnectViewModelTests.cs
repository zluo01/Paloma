using Grpc.Core;
using Paloma.Models;
using Paloma.ViewModels.Settings;
using Xunit;
using Connector = Paloma.Binding.V1.Connector;
using ManualInput = Paloma.Provider.Runtime.V1.ManualInput;
using ProviderAuthMethod = Paloma.Provider.Runtime.V1.ProviderAuthMethod;
using ProviderBackendId = Paloma.Binding.V1.ProviderBackendId;

namespace Paloma.Tests;

// The Challenge and Oauth phases auto-open the system browser from
// StartAsync, so only the Manual and failure paths are exercised here.
public sealed class ConnectViewModelTests
{
    private static Connector TestConnector() =>
        new()
        {
            Id = new ProviderBackendId { ProviderId = "provider", BackendId = "backend" },
            Description = "a test connector",
        };

    [Fact]
    public async Task Start_ManualPhase_WaitsForInputWithoutFinalizing()
    {
        var mock = new MockPalomaClient
        {
            OnInitConnection = _ => new ConnectionPhase.Manual(
                new ManualInput { InstructionsUrl = "https://keys.example" }),
        };
        var vm = new ConnectViewModel(mock, TestConnector());

        await vm.StartAsync();

        Assert.True(vm.IsManual);
        Assert.Empty(mock.FinalizeConnections);
        Assert.Equal("Connect", vm.PrimaryLabel);
    }

    [Fact]
    public async Task Start_WhenInitFails_MapsDetailIntoFailedPhase()
    {
        var mock = new MockPalomaClient
        {
            OnInitConnection = _ =>
                throw new RpcException(new Status(StatusCode.Unavailable, "no credentials")),
        };
        var vm = new ConnectViewModel(mock, TestConnector());

        await vm.StartAsync();

        Assert.True(vm.IsFailed);
        Assert.Contains("no credentials", vm.ErrorMessage);
        Assert.Equal("Close", vm.CloseLabel);
    }

    [Fact]
    public void Input_NonWhitespace_EnablesConnectWithNotification()
    {
        var vm = new ConnectViewModel(new MockPalomaClient(), TestConnector());
        var notified = new List<string?>();
        vm.PropertyChanged += (_, args) => notified.Add(args.PropertyName);

        Assert.False(vm.HasInput);
        vm.Input = "   ";
        Assert.False(vm.HasInput);
        vm.Input = "secret-key";

        Assert.True(vm.HasInput);
        Assert.Contains(nameof(ConnectViewModel.HasInput), notified);
    }

    [Fact]
    public async Task Submit_WhitespaceInput_DoesNothing()
    {
        var mock = new MockPalomaClient
        {
            OnInitConnection = _ => new ConnectionPhase.Manual(new ManualInput()),
        };
        var vm = new ConnectViewModel(mock, TestConnector());
        await vm.StartAsync();

        vm.Input = "   ";
        await vm.SubmitAsync();

        Assert.True(vm.IsManual);
        Assert.Empty(mock.FinalizeConnections);
    }

    [Fact]
    public async Task Submit_ManualInput_FinalizesTrimmedApiKey()
    {
        var mock = new MockPalomaClient
        {
            OnInitConnection = _ => new ConnectionPhase.Manual(new ManualInput()),
        };
        var vm = new ConnectViewModel(mock, TestConnector());
        await vm.StartAsync();

        vm.Input = "  secret-key  ";
        await vm.SubmitAsync();

        var call = Assert.Single(mock.FinalizeConnections);
        Assert.Equal((ProviderAuthMethod.ApiKey, "secret-key"), (call.Method, call.Payload));
        Assert.True(vm.IsSuccess);
        Assert.Equal("Close", vm.CloseLabel);
    }
}