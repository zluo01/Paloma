using Microsoft.UI.Xaml.Controls;
using Paloma.Models;
using Paloma.ViewModels.Settings;

namespace Paloma.Views.Settings.Services;

public sealed partial class ConnectDialog
{
    // Linger so the success state is visible before the dialog closes itself.
    private static readonly TimeSpan SuccessLinger = TimeSpan.FromMilliseconds(800);

    public ConnectViewModel ViewModel { get; }

    public ConnectDialog(ConnectViewModel viewModel)
    {
        ViewModel = viewModel;
        InitializeComponent();
    }

    private async void OnOpened(ContentDialog sender, ContentDialogOpenedEventArgs args)
    {
        await ViewModel.StartAsync();
        await HideOnSuccessAsync();
    }

    private async void OnCloseButtonClick(
        ContentDialog sender,
        ContentDialogButtonClickEventArgs args)
    {
        await ViewModel.CancelAsync();
    }

    private async Task HideOnSuccessAsync()
    {
        if (ViewModel.Phase is ConnectionPhase.Success)
        {
            await Task.Delay(SuccessLinger);
            Hide();
        }
    }

    private async void OnPrimaryButtonClick(
        ContentDialog sender,
        ContentDialogButtonClickEventArgs args)
    {
        args.Cancel = true;
        await ViewModel.SubmitAsync();
        await HideOnSuccessAsync();
    }
}