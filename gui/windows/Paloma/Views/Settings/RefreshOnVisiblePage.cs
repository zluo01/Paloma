using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;

namespace Paloma.Views.Settings;

/// <summary>A cached page that loads on first show and reloads each time
/// the settings window becomes visible again.</summary>
public abstract class RefreshOnVisiblePage : Page
{
    private bool _hostVisible;

    protected RefreshOnVisiblePage()
    {
        NavigationCacheMode = NavigationCacheMode.Required;
        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    protected abstract Task LoadAsync();

    private async void OnLoaded(object sender, RoutedEventArgs args)
    {
        if (XamlRoot is { } root)
        {
            _hostVisible = root.IsHostVisible;
            root.Changed += OnXamlRootChanged;
        }

        await LoadAsync();
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
            await LoadAsync();
        }

        _hostVisible = visible;
    }
}
