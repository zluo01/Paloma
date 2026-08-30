using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Navigation;
using Paloma.Helpers;
using Paloma.ViewModels.Settings;
using Connector = PalomaCore.Connector;

namespace Paloma.Views.Settings.Services;

public sealed partial class ServicesPage
{
    private bool _hostVisible;

    public ServicesViewModel ViewModel { get; }

    public ServicesPage()
    {
        ViewModel = new ServicesViewModel(App.Current.Client);
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

    private void OnConnectClick(object sender, RoutedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext is not Connector connector)
        {
            return;
        }

        _ = ConnectAsync(connector);
    }

    private async Task ConnectAsync(Connector connector)
    {
        var shown = await ClientGuard.TryAsync(
            () => new ConnectDialog(new ConnectViewModel(App.Current.Client, connector))
            {
                XamlRoot = XamlRoot,
            }.TryShowAsync(),
            ViewModel.ReportError,
            "Failed to open the connect dialog");
        if (shown)
        {
            await ViewModel.RefreshAsync();
        }
    }
}