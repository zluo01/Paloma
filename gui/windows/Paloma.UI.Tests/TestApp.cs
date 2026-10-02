using Microsoft.UI.Xaml;

namespace Paloma.UI.Tests;

/// The real app's resources and App.Current, without its startup:
/// no core, tray icon, hotkey or windows.
public sealed class TestApp : App
{
    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
    }
}
