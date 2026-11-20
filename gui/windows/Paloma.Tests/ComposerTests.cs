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
    [InlineData("abc", "abc")]
    [InlineData("a\rb", "a\r\nb")]
    [InlineData("a\vb", "a\r\nb")]
    [InlineData("a\rb\vc\r", "a\r\nb\r\nc\r\n")]
    [InlineData("a \uFFFC b", "a  b")]
    [InlineData("\uFFFC\r\uFFFC", "\r\n")]
    [InlineData("\uFFFC", "")]
    [InlineData("\uFFFC\uFFFC", "")]
    public void GivenTextWithImagesWhenCopyingShouldDropImagesAndUseCrlf(string text, string expected)
    {
        Assert.Equal(expected, Composer.CopyText(text));
    }
}