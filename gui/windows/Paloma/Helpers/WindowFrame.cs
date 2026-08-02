using Windows.Win32;
using Windows.Win32.Foundation;
using Windows.Win32.Graphics.Dwm;

namespace Paloma.Helpers;

internal static class WindowFrame
{
    // Border colors are 0x00BBGGRR, not RGB. Both are gray so a wrong
    // byte order would not be visible.
    private const int DarkShadeBorder = 0x00262626;
    private const int LightShadeBorder = 0x00D9D9D9;

    /// Matches the frame theme and border color to the app theme.
    /// A mismatched frame shows a one pixel light line at the top.
    public static void MatchFrameToTheme(nint hwnd, bool dark)
    {
        SetAttribute(hwnd, DWMWINDOWATTRIBUTE.DWMWA_USE_IMMERSIVE_DARK_MODE, dark ? 1 : 0);
        SetAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE.DWMWA_BORDER_COLOR,
            dark ? DarkShadeBorder : LightShadeBorder);
    }

    public static unsafe void SetAttribute(nint hwnd, DWMWINDOWATTRIBUTE attribute, int value)
    {
        _ = PInvoke.DwmSetWindowAttribute(new HWND(hwnd), attribute, &value, sizeof(int));
    }
}