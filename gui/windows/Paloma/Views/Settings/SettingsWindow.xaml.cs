using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Paloma.Helpers;
using WinRT.Interop;

namespace Paloma.Views.Settings;

public sealed partial class SettingsWindow
{
    private const int SettingsWidth = 920;
    private const int SettingsHeight = 680;
    private const int MinWidth = 760;
    private const int MinHeight = 480;

    public SettingsWindow()
    {
        InitializeComponent();

        // required to make it latest windows native look
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);

        var hwnd = WindowNative.GetWindowHandle(this);
        if (Content is not FrameworkElement root) return;
        WindowFrame.MatchFrameToTheme(hwnd, root.ActualTheme == ElementTheme.Dark);
    }

    private void OnRootThemeChanged(FrameworkElement sender, object args)
    {
        WindowFrame.MatchFrameToTheme(
            WindowNative.GetWindowHandle(this), sender.ActualTheme == ElementTheme.Dark);
    }

    /// <summary>
    /// Move the setting window to current focused monitor and
    /// clamps its minimum size to that monitor's scale.
    /// </summary>
    public void MoveToFocusedMonitor()
    {
        var scale = WindowPlacement.SizeAndCenterOnCursorMonitor(
            AppWindow, SettingsWidth, SettingsHeight);
        if (AppWindow.Presenter is not OverlappedPresenter presenter) return;
        presenter.PreferredMinimumWidth = (int)(MinWidth * scale);
        presenter.PreferredMinimumHeight = (int)(MinHeight * scale);
    }
}