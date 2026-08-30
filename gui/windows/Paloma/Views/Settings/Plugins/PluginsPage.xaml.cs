using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Navigation;
using Paloma.Helpers;
using Paloma.ViewModels.Settings;
using Plugin = PalomaCore.Plugin;
using PluginType = PalomaCore.PluginType;

namespace Paloma.Views.Settings.Plugins;

public sealed partial class PluginsPage
{
    private bool _hostVisible;

    public PluginsViewModel ViewModel { get; }

    public PluginsPage()
    {
        ViewModel = new PluginsViewModel(App.Current.Client);
        NavigationCacheMode = NavigationCacheMode.Required;
        InitializeComponent();
    }

    private async void OnLoaded(object sender, RoutedEventArgs args)
    {
        if (XamlRoot is { } root)
        {
            _hostVisible = root.IsHostVisible;
            root.Changed += OnXamlRootChanged;
        }

        await ViewModel.LoadAsync();
    }

    private void OnUnloaded(object sender, RoutedEventArgs args)
    {
        if (XamlRoot is { } root)
        {
            root.Changed -= OnXamlRootChanged;
        }
    }

    private async void OnXamlRootChanged(XamlRoot root, XamlRootChangedEventArgs args)
    {
        var visible = root.IsHostVisible;
        if (visible && !_hostVisible)
        {
            await ViewModel.LoadAsync();
        }

        _hostVisible = visible;
    }

    private async void OnAddExtension(object sender, RoutedEventArgs args) =>
        await OpenDialogAsync(PluginType.Extension, null);

    private async void OnAddProvider(object sender, RoutedEventArgs args) =>
        await OpenDialogAsync(PluginType.Provider, null);

    private async void OnAddMcp(object sender, RoutedEventArgs args) =>
        await OpenDialogAsync(PluginType.Mcp, null);

    private async void OnEditPlugin(object sender, RoutedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext is PluginViewModel { Config: { } config } plugin)
        {
            await OpenDialogAsync(plugin.Kind, config);
        }
    }

    private async void OnRemovePlugin(object sender, RoutedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext is PluginViewModel plugin)
        {
            await ViewModel.RemoveAsync(plugin);
        }
    }

    private async Task OpenDialogAsync(PluginType kind, Plugin? editing)
    {
        var shown = await RpcGuard.TryAsync(
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