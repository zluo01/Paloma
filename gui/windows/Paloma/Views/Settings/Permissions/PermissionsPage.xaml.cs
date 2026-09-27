using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.ViewModels.Settings;
using Permission = PalomaCore.Permission;

namespace Paloma.Views.Settings.Permissions;

public sealed partial class PermissionsPage
{
    public PermissionsViewModel ViewModel { get; }

    public PermissionsPage()
    {
        ViewModel = new PermissionsViewModel(App.Current.Client);
        InitializeComponent();
    }

    protected override Task LoadAsync() => ViewModel.LoadAsync();

    public static IconElement Icon(string prefix) => new FontIcon
    {
        Glyph = prefix.StartsWith("tool:", StringComparison.Ordinal) ? "\uEC7A" : "\uE756",
    };

    public static string MatchKind(bool withGlob) => withGlob ? "Glob match" : "Exact command";

    private async void OnDeleteClick(object sender, RoutedEventArgs args)
    {
        if (sender is FrameworkElement { DataContext: Permission permission })
        {
            await ViewModel.DeleteAsync(permission);
        }
    }
}