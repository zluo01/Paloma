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

    [Theory]
    [InlineData("", 0, true, true)]
    [InlineData("abc", 1, true, true)]
    [InlineData("ab\rcd", 1, true, false)]
    [InlineData("ab\rcd", 4, false, true)]
    [InlineData("ab\rcd\ref", 4, false, false)]
    [InlineData("ab\r", 3, false, true)]
    [InlineData("ab\rcd\r", 4, false, false)]
    [InlineData("ab\rcd", 2, true, false)]
    [InlineData("ab\rcd", 3, false, true)]
    [InlineData("ab\vcd", 1, true, false)]
    [InlineData("ab\vcd", 4, false, true)]
    [InlineData("ab\vcd\vef", 4, false, false)]
    [InlineData("ab\v", 3, false, true)]
    [InlineData("ab\rcd\vef", 4, false, false)]
    public void GivenCaretWhenCheckingEdgeShouldTellFirstOrLastLine(string text, int caret, bool upEdge, bool downEdge)
    {
        Assert.Equal(upEdge, Composer.CaretOnEdge(-1, text, caret));
        Assert.Equal(downEdge, Composer.CaretOnEdge(1, text, caret));
    }

    // The box counts its final paragraph mark, the text does not.
    [Theory]
    [InlineData("ab\rcd", 6, false, true)]
    [InlineData("ab\rcd", 9, false, true)]
    [InlineData("abc", 5, true, true)]
    [InlineData("", 1, true, true)]
    public void GivenCaretPastTextWhenCheckingEdgeShouldClampToText(string text, int caret, bool upEdge, bool downEdge)
    {
        Assert.Equal(upEdge, Composer.CaretOnEdge(-1, text, caret));
        Assert.Equal(downEdge, Composer.CaretOnEdge(1, text, caret));
    }
}
