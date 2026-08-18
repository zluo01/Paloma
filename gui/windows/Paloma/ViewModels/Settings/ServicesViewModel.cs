using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Paloma.Client;
using Paloma.Extensions;
using Paloma.Helpers;
using Connector = Paloma.Binding.V1.Connector;
using Icon = Paloma.Binding.V1.Icon;
using ProviderBackendId = Paloma.Binding.V1.ProviderBackendId;

namespace Paloma.ViewModels.Settings;

public sealed partial class ServicesViewModel(IPalomaClient client) : ObservableObject
{
    public ObservableCollection<ConnectorViewModel> Connected { get; } = [];

    public ObservableCollection<Connector> Available { get; } = [];

    [ObservableProperty] public partial bool Loading { get; private set; } = true;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasError))]
    public partial string Error { get; private set; } = string.Empty;

    public bool HasError => Error.Length > 0;

    public void ReportError(string message) => Error = message;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowNoConnected))]
    [NotifyPropertyChangedFor(nameof(ShowNoAvailable))]
    public partial bool Ready { get; private set; }

    public bool ShowNoConnected => Ready && Connected.Count == 0;

    public bool ShowNoAvailable => Ready && Available.Count == 0;

    public async Task LoadAsync()
    {
        if (await RefreshAsync())
        {
            Ready = true;
        }

        Loading = false;
    }

    public async Task<bool> RefreshAsync()
    {
        var refreshed = await RpcGuard.TryAsync(
            async () =>
            {
                var connectors = await client.GetConnectorsAsync();
                Connected.Clear();
                Available.Clear();
                foreach (var connector in connectors)
                {
                    if (connector.Connection is not null)
                    {
                        Connected.Add(new ConnectorViewModel(
                            client, connector, RefreshAsync, message => Error = message));
                    }
                    else
                    {
                        Available.Add(connector);
                    }
                }

                OnPropertyChanged(nameof(ShowNoConnected));
                OnPropertyChanged(nameof(ShowNoAvailable));
            },
            message => Error = message,
            "Refreshing services failed");
        if (refreshed)
        {
            Error = string.Empty;
        }

        return refreshed;
    }
}

public sealed partial class ConnectorViewModel(
    IPalomaClient client,
    Connector connector,
    Func<Task> refresh,
    Action<string> report)
{
    private ProviderBackendId Id { get; } = connector.Id;

    public string BackendLabel { get; } = Display.Backend(connector.Id);

    public string Description { get; } = connector.Description;

    public Icon? Icon { get; } = connector.Icon;

    public string? Error { get; } =
        connector.Connection!.Status is { HasError: true } status ? status.Error : null;

    public bool HasError => Error is { Length: > 0 };

    [RelayCommand]
    private async Task DisconnectAsync()
    {
        if (await RpcGuard.TryAsync(
                () => client.DisconnectAsync(Id),
                report,
                "Failed to disconnect"))
        {
            await refresh();
        }
    }
}