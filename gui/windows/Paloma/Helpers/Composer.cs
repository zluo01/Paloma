using System.Text;

namespace Paloma.Helpers;

internal static class Composer
{
    public static string QuotePath(string path)
    {
        return $"\"{path}\"";
    }

    public static string Prompt(string text, int imagesBefore = 0)
    {
        var prompt = new StringBuilder(text.Length);
        var count = imagesBefore;
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
        return text
            .Replace(Images.ImageCharacter.ToString(), string.Empty, StringComparison.Ordinal)
            .Replace("\r", "\r\n", StringComparison.Ordinal)
            .Replace("\v", "\r\n", StringComparison.Ordinal);
    }
}