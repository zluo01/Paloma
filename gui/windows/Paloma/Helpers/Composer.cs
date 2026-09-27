namespace Paloma.Helpers;

internal static class Composer
{
    public static bool CaretOnEdge(int delta, string text, int caret)
    {
        return delta < 0
            ? text.AsSpan(0, caret).IndexOfAny('\r', '\v') < 0
            : text.AsSpan(caret).IndexOfAny('\r', '\v') < 0;
    }
}
