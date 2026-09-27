using Paloma.Helpers;
using Xunit;

namespace Paloma.Tests;

public sealed class ComposerTests
{
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
    public void CaretOnEdge_IsTheFirstOrLastTextLine(string text, int caret, bool upEdge, bool downEdge)
    {
        Assert.Equal(upEdge, Composer.CaretOnEdge(-1, text, caret));
        Assert.Equal(downEdge, Composer.CaretOnEdge(1, text, caret));
    }

    [Theory]
    [InlineData(@"C:\a.txt", @"""C:\a.txt""")]
    [InlineData(@"C:\my docs\a b.pdf", @"""C:\my docs\a b.pdf""")]
    [InlineData(@"\\server\share\x.log", @"""\\server\share\x.log""")]
    [InlineData(@"C:\folder", @"""C:\folder""")]
    [InlineData(@"C:\it's (1) & more.txt", @"""C:\it's (1) & more.txt""")]
    public void QuotePath_WrapsThePathInDoubleQuotes(string path, string expected)
    {
        Assert.Equal(expected, Composer.QuotePath(path));
    }
}
