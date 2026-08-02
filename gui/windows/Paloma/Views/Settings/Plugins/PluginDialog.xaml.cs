using Microsoft.UI.Xaml.Controls;
using Paloma.ViewModels.Settings;

namespace Paloma.Views.Settings.Plugins;

public sealed partial class PluginDialog
{
    public PluginDialogViewModel ViewModel { get; }

    public PluginDialog(PluginDialogViewModel viewModel)
    {
        ViewModel = viewModel;
        InitializeComponent();
    }

    private void OnClosing(ContentDialog sender, ContentDialogClosingEventArgs args)
    {
        ViewModel.Cancel();
    }

    private async void OnPrimaryButtonClick(
        ContentDialog sender,
        ContentDialogButtonClickEventArgs args)
    {
        // Always canceled: only a successful submit closes the dialog, and
        // failures keep the form up with the error shown.
        args.Cancel = true;
        if (await ViewModel.SubmitAsync())
        {
            Hide();
        }
    }
}