using Windows.Graphics;
using Windows.Win32;
using Windows.Win32.Graphics.Gdi;
using Windows.Win32.UI.HiDpi;
using Microsoft.UI;
using Microsoft.UI.Windowing;

namespace Paloma.Helpers;

internal static class WindowPlacement
{
    // Default to center the window at the golden ratio above the work area bottom.
    private const double GoldenCenterFromTop = 0.382;

    /// Centers the window on the cursor monitor at that monitor's scale. Returns the scale.
    public static double SizeAndCenterOnCursorMonitor(
        AppWindow window,
        double logicalWidth,
        double logicalHeight)
    {
        var (area, scale) = CursorMonitor();
        var width = (int)(logicalWidth * scale);
        var height = (int)(logicalHeight * scale);
        // Move before resize. Crossing into a different DPI monitor
        // rescales the window during the move.
        window.Move(new PointInt32(
            area.X + (area.Width - width) / 2,
            area.Y + (area.Height - height) / 2));
        window.Resize(new SizeInt32(width, height));
        return scale;
    }

    /// Sizes the window for the cursor monitor and places it at the golden
    /// ratio spot. With keepIfOnMonitor a window already on that monitor
    /// keeps its place.
    public static void SizeAndPlaceOnCursorMonitor(AppWindow window,
        double logicalWidth,
        double logicalHeight,
        bool keepIfOnMonitor)
    {
        var (area, scale) = CursorMonitor();
        var width = (int)(logicalWidth * scale);
        var height = (int)(logicalHeight * scale);
        var position = window.Position;
        var size = window.Size;
        var centerX = position.X + size.Width / 2;
        var centerY = position.Y + size.Height / 2;
        var keep = keepIfOnMonitor
                   && centerX >= area.X
                   && centerX < area.X + area.Width
                   && centerY >= area.Y
                   && centerY < area.Y + area.Height;
        if (!keep)
        {
            // Move before resize. Crossing into a different DPI monitor
            // rescales the window during the move.
            window.Move(new PointInt32(
                area.X + (area.Width - width) / 2,
                area.Y + (int)(area.Height * GoldenCenterFromTop) - height / 2));
        }

        window.Resize(new SizeInt32(width, height));
    }

    /// Work area and scale of the monitor holding the cursor.
    /// The cursor monitor can differ from the one the window sits on.
    private static (RectInt32 WorkArea, double Scale) CursorMonitor()
    {
        PInvoke.GetCursorPos(out var point);
        var display = DisplayArea.GetFromPoint(
            new PointInt32(point.X, point.Y), DisplayAreaFallback.Nearest);
        var monitor = new HMONITOR(Win32Interop.GetMonitorFromDisplayId(display.DisplayId));
        var scale = PInvoke.GetDpiForMonitor(
            monitor,
            MONITOR_DPI_TYPE.MDT_EFFECTIVE_DPI,
            out var dpi,
            out _).Succeeded
            ? dpi / 96.0
            : 1.0;
        return (display.WorkArea, scale);
    }
}