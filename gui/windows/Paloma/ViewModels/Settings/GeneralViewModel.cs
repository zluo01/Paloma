using Windows.System;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using CommunityToolkit.Mvvm.ComponentModel;
using Paloma.Models;
using Paloma.Settings;

namespace Paloma.ViewModels.Settings;

public sealed partial class GeneralViewModel : ObservableObject
{
    private readonly ToggleGuard _autostart;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasError))]
    public partial string Error { get; private set; } = string.Empty;

    [ObservableProperty] public partial bool AutostartEnabled { get; set; }

    [ObservableProperty] public partial string ShortcutLabel { get; private set; }

    public GeneralViewModel()
    {
        _autostart = new ToggleGuard(
            "autostart",
            value =>
            {
                AppSettings.SetAutostart(value);
                return Task.CompletedTask;
            },
            value => AutostartEnabled = value,
            message => Error = message);
        AutostartEnabled = AppSettings.IsAutostartEnabled();
        ShortcutLabel = App.Current.Settings.HotKeyLabel;
        _autostart.Ready();
    }

    public bool HasError => Error.Length > 0;

    public void BeginShortcutRecording()
    {
        ShortcutLabel = "Press a key combination…";
        Error = string.Empty;
    }

    public void EndShortcutRecording()
    {
        ShortcutLabel = App.Current.Settings.HotKeyLabel;
    }

    public bool TryBindHotKey(HOT_KEY_MODIFIERS modifiers, VirtualKey key)
    {
        if (modifiers == 0)
        {
            Error = "Use at least one modifier (Ctrl, Alt, Shift, or Win).";
            return false;
        }

        try
        {
            App.Current.Settings.UpdateHotKey(new KeyBinding(modifiers, key));
            Error = string.Empty;
            return true;
        }
        catch (Exception e)
        {
            Error = e.Message;
            return false;
        }
    }

    partial void OnAutostartEnabledChanged(bool value) => _autostart.Changed(value);
}