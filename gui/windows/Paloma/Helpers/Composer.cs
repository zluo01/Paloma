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
}
