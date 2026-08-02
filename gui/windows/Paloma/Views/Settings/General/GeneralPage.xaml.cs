using Windows.System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Navigation;
using Paloma.Helpers;
using Paloma.ViewModels.Settings;

namespace Paloma.Views.Settings.General;

public sealed partial class GeneralPage
{
    public GeneralViewModel ViewModel { get; }

    public GeneralPage()
    {
        ViewModel = new GeneralViewModel();
        NavigationCacheMode = NavigationCacheMode.Required;
        InitializeComponent();
    }

    private void OnRecordChecked(object sender, RoutedEventArgs args)
    {
        ViewModel.BeginShortcutRecording();
        RecordButton.Focus(FocusState.Programmatic);
    }

    private void OnRecordUnchecked(object sender, RoutedEventArgs args)
    {
        ViewModel.EndShortcutRecording();
    }

    private void OnRecordLostFocus(object sender, RoutedEventArgs args)
    {
        RecordButton.IsChecked = false;
    }

    private void OnRecordKeyDown(object sender, KeyRoutedEventArgs args)
    {
        if (RecordButton.IsChecked != true)
        {
            return;
        }
        args.Handled = true;
        var key = args.Key;
        switch (key)
        {
            case VirtualKey.Shift or VirtualKey.Control or VirtualKey.Menu
                or VirtualKey.LeftWindows or VirtualKey.RightWindows:
                return;
            case VirtualKey.Escape:
                RecordButton.IsChecked = false;
                return;
        }

        if (ViewModel.TryBindHotKey(Keyboard.GetPressedModifiers(), key))
        {
            RecordButton.IsChecked = false;
        }
    }
}
