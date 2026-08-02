using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Paloma.Helpers;

/// Scrolls the selected row into view without animation: animated
/// requests stack under key repeat and the stale ones settle last
internal static class RowScroller
{
    public static void BringIntoView(
        ScrollViewer scroller,
        FrameworkElement? container,
        int index,
        int count)
    {
        // The extremes snap the whole list, so leading content like a
        // section header stays visible.
        if (index <= 0)
        {
            scroller.ChangeView(null, 0, null, true);
        }
        else if (index == count - 1)
        {
            scroller.ChangeView(null, scroller.ScrollableHeight, null, true);
        }
        else if (container is not null)
        {
            ScrollRowIntoView(scroller, container);
        }
    }

    private static void ScrollRowIntoView(ScrollViewer scroller, FrameworkElement container)
    {
        var top = container.TransformToVisual(scroller)
            .TransformPoint(default).Y + scroller.VerticalOffset;
        var bottom = top + container.ActualHeight;
        if (top < scroller.VerticalOffset)
        {
            scroller.ChangeView(null, top - 4, null, true);
        }
        else if (bottom > scroller.VerticalOffset + scroller.ViewportHeight)
        {
            scroller.ChangeView(null, bottom - scroller.ViewportHeight + 4, null, true);
        }
    }
}