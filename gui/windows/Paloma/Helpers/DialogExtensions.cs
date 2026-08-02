using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Paloma.Helpers;

public static class DialogExtensions
{
    /// <summary>Shows the dialog and returns true, or returns false without
    /// showing when another ContentDialog is already open on this XamlRoot.
    /// WinUI allows one open ContentDialog per UI thread.</summary>
    public static async Task<bool> TryShowAsync(this ContentDialog dialog)
    {
        if (VisualTreeHelper.GetOpenPopupsForXamlRoot(dialog.XamlRoot)
            .Any(popup => popup.Child is ContentDialog))
        {
            return false;
        }
        await dialog.ShowAsync();
        return true;
    }
}
