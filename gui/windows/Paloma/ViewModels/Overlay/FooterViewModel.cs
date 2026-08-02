using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Messaging;
using Paloma.Client;
using Paloma.Helpers;
using Paloma.Messages;
using Connector = Paloma.Binding.V1.Connector;
using HealthLevel = Paloma.Binding.V1.HealthLevel;
using HealthStatus = Paloma.Binding.V1.HealthStatus;
using Model = Paloma.Provider.Runtime.V1.Model;
using ProviderBackendId = Paloma.Binding.V1.ProviderBackendId;

namespace Paloma.ViewModels.Overlay;

public sealed partial class FooterViewModel(IPalomaClient client, IMessenger? messenger = null)
    : ObservableObject
{
    private readonly IMessenger _messenger = messenger ?? WeakReferenceMessenger.Default;

    [ObservableProperty] public partial HealthLevel ServicesHealth { get; private set; } = HealthLevel.Inactive;

    [ObservableProperty] public partial HealthLevel PluginsHealth { get; private set; } = HealthLevel.Inactive;

    public IReadOnlyList<Connector> Connected { get; private set; } = [];

    [ObservableProperty] public partial bool HasSelectableProvider { get; private set; }

    [ObservableProperty] public partial string ModelLabel { get; private set; } = "No model";

    [ObservableProperty] public partial string SelectedModelId { get; private set; } = string.Empty;

    [ObservableProperty] public partial string SelectedEffort { get; private set; } = string.Empty;

    public async Task RefreshAsync()
    {
        if (!await RpcGuard.TryAsync(
                async () =>
                {
                    (ServicesHealth, PluginsHealth) = await client.GetHealthAsync();
                    await RefreshModelsAsync();
                },
                Report,
                "Refresh failed"))
        {
            ServicesHealth = HealthLevel.Down;
            PluginsHealth = HealthLevel.Down;
            ClearModels();
        }
    }

    public async Task SelectModelAsync(ProviderBackendId backend, Model model, string effort)
    {
        SelectedModelId = model.Id;
        SelectedEffort = effort;
        ModelLabel = Label(model, effort);
        await RpcGuard.TryAsync(
            () => client.SetModelPreferenceAsync(
                backend,
                model.Id,
                effort,
                asDefault: true),
            Report,
            "Failed to set model");
    }

    private async Task RefreshModelsAsync()
    {
        var connectors = await client.GetConnectorsAsync();
        Connected = [.. connectors.Where(connector => connector.Connection is not null)];
        HasSelectableProvider = Connected.Any(connector =>
            connector.Connection is { Status: { Status: HealthStatus.Running } status }
            && status.Models.Any(model => model.SupportedReasoningEfforts.Count > 0));

        // A preference only counts while its provider runs and still offers
        // the model and effort. Anything stale shows "Select model" instead
        // of guessing for the user.
        (ProviderBackendId Backend, Model Model, string Effort)? selection = null;
        foreach (var connector in Connected)
        {
            if (connector.Connection is not
                { Status: { Status: HealthStatus.Running } status, Preferred: true } connection)
            {
                continue;
            }

            var model = status.Models.FirstOrDefault(m => m.Id == connection.PreferModel);
            if (model is null || !model.SupportedReasoningEfforts.Contains(connection.PreferEffort)) continue;
            selection = (connector.Id, model, connection.PreferEffort);
            break;
        }

        if (selection is not { } chosen)
        {
            ClearSelection();
            return;
        }

        SelectedModelId = chosen.Model.Id;
        SelectedEffort = chosen.Effort;
        ModelLabel = Label(chosen.Model, chosen.Effort);
    }

    private void ClearModels()
    {
        Connected = [];
        HasSelectableProvider = false;
        ClearSelection();
    }

    private void ClearSelection()
    {
        SelectedModelId = string.Empty;
        SelectedEffort = string.Empty;
        ModelLabel = HasSelectableProvider ? "Select model" : "No model";
    }

    private void Report(string message)
    {
        _messenger.Send(new ErrorReportedMessage(message));
    }

    private static string Label(Model model, string effort) =>
        model.SupportedReasoningEfforts.Count > 1 ? $"{model.Name} · {effort}" : model.Name;
}