using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using Paloma.ViewModels.Settings;
using Permission = Paloma.Binding.V1.Permission;

namespace Paloma.Views.Settings.Permissions;

public sealed partial class PermissionsPage
{
    private bool _hostVisible;

    public PermissionsViewModel ViewModel { get; }

    public PermissionsPage()
    {
        ViewModel = new PermissionsViewModel(App.Current.Client);
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

    public static string Glyph(string prefix) =>
        prefix.StartsWith("tool:", StringComparison.Ordinal) ? "\uEC7A" : "\uE756";

    private void OnFilterChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args)
    {
        ViewModel.Filter = sender.Text;
    }

    private async void OnDeleteClick(object sender, RoutedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext is Permission permission)
        {
            await ViewModel.DeleteAsync(permission);
        }
    }
}