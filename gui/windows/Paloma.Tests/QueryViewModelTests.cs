using Paloma.ViewModels.Overlay;
using Xunit;

namespace Paloma.Tests;

public sealed class QueryViewModelTests
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
        Assert.Equal(upEdge, QueryViewModel.CaretOnEdge(-1, text, caret));
        Assert.Equal(downEdge, QueryViewModel.CaretOnEdge(1, text, caret));
    }

    // The box counts its final paragraph mark, the text does not.
    [Theory]
    [InlineData("ab\rcd", 6, false, true)]
    [InlineData("ab\rcd", 9, false, true)]
    [InlineData("abc", 5, true, true)]
    [InlineData("", 1, true, true)]
    public void CaretOnEdge_ClampsTheCaretToTheText(string text, int caret, bool upEdge, bool downEdge)
    {
        Assert.Equal(upEdge, QueryViewModel.CaretOnEdge(-1, text, caret));
        Assert.Equal(downEdge, QueryViewModel.CaretOnEdge(1, text, caret));
    }
}
