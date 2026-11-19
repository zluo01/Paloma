using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.ViewModels.Settings;

namespace Paloma.Views.Settings.Plugins;

public sealed partial class PluginDialog
{
    private const double WindowMargin = 32;

    public PluginDialogViewModel ViewModel { get; }

    public PluginDialog(PluginDialogViewModel viewModel)
    {
        ViewModel = viewModel;
        InitializeComponent();
    }

    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        if (GetTemplateChild("BackgroundElement") is FrameworkElement card)
        {
            card.Margin = new Thickness(0, WindowMargin, 0, WindowMargin);
        }
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