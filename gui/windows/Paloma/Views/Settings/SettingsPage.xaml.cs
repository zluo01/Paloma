using Microsoft.UI.Xaml.Controls;
using Paloma.Views.Settings.General;
using Paloma.Views.Settings.Permissions;
using Paloma.Views.Settings.Plugins;
using Paloma.Views.Settings.Services;
using Paloma.Views.Settings.Shortcuts;

namespace Paloma.Views.Settings;

public sealed partial class SettingsPage
{
    public SettingsPage()
    {
        InitializeComponent();
    }

    private void OnSelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        var tag = (args.SelectedItem as NavigationViewItem)?.Tag as string;
        var page = tag switch
        {
            "general" => typeof(GeneralPage),
            "plugins" => typeof(PluginsPage),
            "permissions" => typeof(PermissionsPage),
            "shortcuts" => typeof(ShortcutsPage),
            _ => typeof(ServicesPage),
        };
        if (ContentFrame.CurrentSourcePageType != page)
        {
            ContentFrame.Navigate(page);
        }
    }
}