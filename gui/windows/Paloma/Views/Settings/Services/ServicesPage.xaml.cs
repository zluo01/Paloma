using Microsoft.UI.Xaml;
using Paloma.Helpers;
using Paloma.ViewModels.Settings;
using Connector = PalomaCore.Connector;

namespace Paloma.Views.Settings.Services;

public sealed partial class ServicesPage
{
    public ServicesViewModel ViewModel { get; }

    public ServicesPage()
    {
        ViewModel = new ServicesViewModel(App.Current.Client);
        InitializeComponent();
    }

    protected override Task LoadAsync() => ViewModel.LoadAsync();

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