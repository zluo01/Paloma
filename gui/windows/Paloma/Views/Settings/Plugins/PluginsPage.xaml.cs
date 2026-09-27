using Microsoft.UI.Xaml;
using Paloma.Helpers;
using Paloma.ViewModels.Settings;
using Plugin = PalomaCore.Plugin;
using PluginType = PalomaCore.PluginType;

namespace Paloma.Views.Settings.Plugins;

public sealed partial class PluginsPage
{
    public PluginsViewModel ViewModel { get; }

    public PluginsPage()
    {
        ViewModel = new PluginsViewModel(App.Current.Client);
        InitializeComponent();
    }

    protected override Task LoadAsync() => ViewModel.LoadAsync();

    private async void OnAddExtension(object sender, RoutedEventArgs args) =>
        await OpenDialogAsync(PluginType.Extension, null);

    private async void OnAddProvider(object sender, RoutedEventArgs args) =>
        await OpenDialogAsync(PluginType.Provider, null);

    private async void OnAddMcp(object sender, RoutedEventArgs args) =>
        await OpenDialogAsync(PluginType.Mcp, null);

    private async void OnEditPlugin(object sender, RoutedEventArgs args)
    {
        if (sender is FrameworkElement { DataContext: PluginViewModel { Config: { } config } plugin })
        {
            await OpenDialogAsync(plugin.Kind, config);
        }
    }

    private async void OnRemovePlugin(object sender, RoutedEventArgs args)
    {
        if (sender is FrameworkElement { DataContext: PluginViewModel plugin })
        {
            await ViewModel.RemoveAsync(plugin);
        }
    }

    private async Task OpenDialogAsync(PluginType kind, Plugin? editing)
    {
        var shown = await ClientGuard.TryAsync(
            async () =>
            {
                using var model = new PluginDialogViewModel(
                    App.Current.Client, ViewModel.TakenNames(), kind, editing);
                return await new PluginDialog(model)
                {
                    XamlRoot = XamlRoot,
                }.TryShowAsync();
            },
            message => ViewModel.Status = message,
            "Failed to open the plugin dialog");
        if (shown)
        {
            await ViewModel.LoadAsync();
        }
    }
}