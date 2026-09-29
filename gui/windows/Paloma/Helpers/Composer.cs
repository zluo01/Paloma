using System.Text;

namespace Paloma.Helpers;

internal static class Composer
{
    public static string QuotePath(string path)
    {
        return $"\"{path}\"";
    }

    public static string Prompt(string text)
    {
        var prompt = new StringBuilder(text.Length);
        var count = 0;
        foreach (var character in text)
        {
            if (character != Images.ImageCharacter)
            {
                prompt.Append(character);
                continue;
            }

            prompt.Append("[Image #").Append(++count).Append(']');
        }

        return prompt.ToString();
    }

    public static string CopyText(string text)
    {
        return Prompt(text)
            .Replace("\r", "\r\n", StringComparison.Ordinal)
            .Replace("\v", "\r\n", StringComparison.Ordinal);
    }

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
