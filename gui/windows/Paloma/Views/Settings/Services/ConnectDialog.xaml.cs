using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Paloma.Models;
using Paloma.ViewModels.Settings;
using Instruction = PalomaCore.Instruction;

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
        ViewModel.PropertyChanged += OnViewModelPropertyChanged;
    }

    private void OnViewModelPropertyChanged(object? sender, PropertyChangedEventArgs args)
    {
        if (string.IsNullOrEmpty(args.PropertyName)
            || args.PropertyName == nameof(ConnectViewModel.Instructions))
        {
            RenderInstructions();
        }
    }

    private void RenderInstructions()
    {
        var inlines = InstructionsText.Inlines;
        inlines.Clear();
        var secondary = (Brush)Application.Current.Resources["TextFillColorSecondaryBrush"];
        foreach (var instruction in ViewModel.Instructions)
        {
            switch (instruction)
            {
                case Instruction.Text text:
                    inlines.Add(new Run { Text = text.TextValue, Foreground = secondary });
                    break;
                case Instruction.Link link:
                    RenderLink(inlines, link.Label, link.LinkValue, secondary);
                    break;
            }
        }
    }

    private static void RenderLink(
        InlineCollection target,
        string label,
        string url,
        Brush fallbackForeground)
    {
        if (Uri.TryCreate(url, UriKind.Absolute, out var uri))
        {
            var hyperlink = new Hyperlink { NavigateUri = uri };
            hyperlink.Inlines.Add(new Run { Text = label });
            target.Add(hyperlink);
        }
        else
        {
            target.Add(new Run { Text = label, Foreground = fallbackForeground });
        }
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