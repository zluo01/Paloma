namespace Paloma.ViewModels.Overlay;

public sealed class QueryViewModel
{
    // Check if the caret is either top or bottom
    // when in multiple line, we only move the result with arrow keys when caret is moved.
    public static bool CaretOnEdge(int delta, string text, int caret)
    {
        var position = Math.Min(caret, text.Length);
        if (delta < 0)
        {
            return text.AsSpan(0, position).IndexOfAny('\r', '\v') < 0;
        }

        return text.AsSpan(position).IndexOfAny('\r', '\v') < 0;
    }
}
