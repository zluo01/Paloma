using Paloma.Helpers;
using Xunit;

namespace Paloma.Tests;

public sealed class ComposerTests
{
    [Theory]
    [InlineData(@"C:\a.txt", @"""C:\a.txt""")]
    [InlineData(@"C:\my docs\a b.pdf", @"""C:\my docs\a b.pdf""")]
    [InlineData(@"\server\share\x.log", @"""\server\share\x.log""")]
    [InlineData(@"C:\folder", @"""C:\folder""")]
    [InlineData(@"C:\it's (1) & more.txt", @"""C:\it's (1) & more.txt""")]
    public void QuotePath_WrapsThePathInDoubleQuotes(string path, string expected)
    {
        Assert.Equal(expected, Composer.QuotePath(path));
    }

    [Theory]
    [InlineData("", "")]
    [InlineData("abc", "abc")]
    [InlineData("\uFFFCabc", "[Image #1]abc")]
    [InlineData("a\uFFFCb\uFFFCc", "a[Image #1]b[Image #2]c")]
    [InlineData("\uFFFC\uFFFC", "[Image #1][Image #2]")]
    [InlineData("a \uFFFC b", "a [Image #1] b")]
    [InlineData("a\r\uFFFC", "a\r[Image #1]")]
    public void GivenTextWithImagesWhenBuildingPromptShouldNumberImagePlaceholders(string text, string expected)
    {
        Assert.Equal(expected, Composer.Prompt(text));
    }

    [Theory]
    [InlineData("abc", "abc")]
    [InlineData("a\rb", "a\r\nb")]
    [InlineData("a\vb", "a\r\nb")]
    [InlineData("a\rb\vc\r", "a\r\nb\r\nc\r\n")]
    [InlineData("a \uFFFC b", "a [Image #1] b")]
    [InlineData("\uFFFC\r\uFFFC", "[Image #1]\r\n[Image #2]")]
    public void GivenTextWithImagesWhenCopyingShouldNumberImagesAndUseCrlf(string text, string expected)
    {
        Assert.Equal(expected, Composer.CopyText(text));
    }
}
