using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Paloma.Client;
using Paloma.Extensions;
using Paloma.Helpers;
using Connector = Paloma.Binding.V1.Connector;
using Icon = Paloma.Binding.V1.Icon;
using Model = Paloma.Provider.Runtime.V1.Model;
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
    Action<string> report) : ObservableObject
{
    private bool _switchingModel;

    private ProviderBackendId Id { get; } = connector.Id;

    public string Description { get; } = connector.Description;

    public string BackendLabel { get; } = Display.Backend(connector.Id);

    public string? Error { get; } =
        connector.Connection!.Status is { HasError: true } status ? status.Error : null;

    public Icon? Icon { get; } = connector.Icon;

    public IReadOnlyList<Model> Models { get; } = connector.Connection!.Status?.Models ?? [];

    public bool ShowPickers { get; } =
        connector.Connection!.Status is { Error: not { Length: > 0 } } liveStatus
        && liveStatus.Models.Count > 0;

    // Initializers write the backing fields directly, so seeding from the
    // stored preferences never runs the change hooks or their persists.
    [ObservableProperty] public partial Model? SelectedModel { get; set; } = InitialModel(connector);

    [ObservableProperty]
    public partial IReadOnlyList<string> Efforts { get; private set; } =
        InitialModel(connector)?.SupportedReasoningEfforts ?? [];

    [ObservableProperty] public partial string? SelectedEffort { get; set; } = InitialEffort(connector);

    partial void OnSelectedModelChanged(Model? value)
    {
        if (value is null)
        {
            return;
        }

        Efforts = value.SupportedReasoningEfforts;
        // Persisted here, not via the effort setter: when the new model's
        // default effort equals the current one, no effort change fires.
        _switchingModel = true;
        SelectedEffort = value.DefaultReasoningEffort;
        _switchingModel = false;
        Persist();
    }

    partial void OnSelectedEffortChanged(string? value)
    {
        if (_switchingModel || SelectedModel is null || value is null)
        {
            return;
        }

        Persist();
    }

    private async void Persist()
    {
        if (SelectedModel is not { } model || SelectedEffort is not { } effort)
        {
            return;
        }

        await RpcGuard.TryAsync(
            () => client.SetModelPreferenceAsync(Id, model.Id, effort),
            report,
            "Failed to set model");
    }

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

    private static Model? InitialModel(Connector connector)
    {
        var connection = connector.Connection!;
        var models = connection.Status?.Models ?? [];
        return models.FirstOrDefault(model => model.Id == connection.PreferModel)
               ?? (models.Count > 0 ? models[0] : null);
    }

    private static string? InitialEffort(Connector connector)
    {
        var model = InitialModel(connector);
        var efforts = model?.SupportedReasoningEfforts ?? [];
        return efforts.Contains(connector.Connection!.PreferEffort)
            ? connector.Connection!.PreferEffort
            : model?.DefaultReasoningEffort;
    }
}