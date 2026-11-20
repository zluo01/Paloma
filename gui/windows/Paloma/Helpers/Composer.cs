namespace Paloma.Helpers;

internal static class Composer
{
    public static string QuotePath(string path)
    {
        return $"\"{path}\"";
    }

    public static string CopyText(string text)
    {
        return text
            .Replace(Images.ImageCharacter.ToString(), string.Empty, StringComparison.Ordinal)
            .Replace("\r", "\r\n", StringComparison.Ordinal)
            .Replace("\v", "\r\n", StringComparison.Ordinal);
    }
}