using CommunityToolkit.WinUI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Paloma.Views.Overlay.Query;
using Xunit;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed class QueryViewTests(UiFixture ui)
{
    private const int Up = -1;
    private const int Down = 1;

    // Wraps to four lines at the overlay width, with no line breaks.
    private static readonly string Paragraph = string.Join(" ", Enumerable.Repeat("one long paragraph that wraps", 8));

    [Fact]
    public Task GivenTextWhenSettingShouldReadItBack() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });

        query.Text = "hello";

        Assert.Equal("hello", query.Text);
    });

    [Fact]
    public Task GivenSelectionWhenFocusingInputShouldKeepIt() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        var input = query.FindDescendant<RichEditBox>()!;
        query.Text = "please summarize this file and then list the risks";
        input.Document.Selection.SetRange(7, 16);

        query.FocusInput();
        await ui.IdleAsync();

        Assert.Equal((7, 16), (input.Document.Selection.StartPosition, input.Document.Selection.EndPosition));
    });

    [Fact]
    public Task GivenCaretInTheMiddleWhenFocusingInputShouldKeepIt() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        var input = query.FindDescendant<RichEditBox>()!;
        query.Text = "please summarize this file and then list the risks";
        input.Document.Selection.SetRange(7, 7);

        query.FocusInput();
        await ui.IdleAsync();

        Assert.Equal((7, 7), (input.Document.Selection.StartPosition, input.Document.Selection.EndPosition));
    });

    [Fact]
    public Task GivenBackwardSelectionWhenFocusingInputShouldKeepItsDirection() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        var selection = query.FindDescendant<RichEditBox>()!.Document.Selection;
        query.Text = "please summarize this file and then list the risks";
        selection.SetRange(16, 7);
        Assert.True(selection.Options.HasFlag(SelectionOptions.StartActive));

        query.FocusInput();
        await ui.IdleAsync();

        Assert.Equal((7, 16), (selection.StartPosition, selection.EndPosition));
        Assert.True(selection.Options.HasFlag(SelectionOptions.StartActive));
    });

    [Fact]
    public Task GivenSelectionWhenFocusingInputToTheEndShouldMoveCaretToTheEnd() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        var input = query.FindDescendant<RichEditBox>()!;
        query.Text = "please summarize this file and then list the risks";
        input.Document.Selection.SetRange(7, 16);

        query.FocusInput(moveCaretToEnd: true);
        await ui.IdleAsync();

        Assert.Equal((50, 50), (input.Document.Selection.StartPosition, input.Document.Selection.EndPosition));
    });

    [Fact]
    public Task GivenInputWhenFocusingShouldGiveItKeyboardFocus() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        var input = query.FindDescendant<RichEditBox>()!;

        query.FocusInput();
        await ui.IdleAsync();

        Assert.Same(input, FocusManager.GetFocusedElement(input.XamlRoot));
        Assert.Equal(FocusState.Keyboard, input.FocusState);
    });

    [Fact]
    public Task GivenWrappedParagraphWhenCaretIsOnTheLastLineShouldOnlyBeOnTheBottomEdge() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(Paragraph);
        Select(input, Paragraph.Length, Paragraph.Length);
        Assert.Equal(3, CaretLine(input));

        Assert.Equal((false, true), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenWrappedParagraphWhenCaretIsOnTheFirstLineShouldOnlyBeOnTheTopEdge() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(Paragraph);
        Select(input, 5, 5);
        Assert.Equal(0, CaretLine(input));

        Assert.Equal((true, false), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenWrappedParagraphWhenCaretIsOnAMiddleLineShouldBeOnNeitherEdge() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(Paragraph);
        var caret = FirstPositionOnLine(input, 1) + 5;
        Select(input, caret, caret);
        Assert.Equal(1, CaretLine(input));

        Assert.Equal((false, false), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenWrappedParagraphWhenCaretIsAtTheStartOfTheSecondLineShouldBeOnNeitherEdge() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(Paragraph);
        var caret = FirstPositionOnLine(input, 1);
        Select(input, caret, caret);
        Assert.Equal(1, CaretLine(input));

        Assert.Equal((false, false), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenSelectionWithinTheMiddleLinesWhenCheckingEdgesShouldBeOnNeither() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(Paragraph);
        Select(input, FirstPositionOnLine(input, 1) + 5, FirstPositionOnLine(input, 2) + 5);

        Assert.Equal((false, false), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenSelectionFromTheFirstToTheLastLineWhenCheckingEdgesShouldBeOnBoth() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(Paragraph);
        Select(input, 5, Paragraph.Length - 5);

        Assert.Equal((true, true), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenSingleLineWhenCheckingEdgesShouldBeOnBoth() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync("hello");
        Select(input, 2, 2);

        Assert.Equal((true, true), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenEmptyInputWhenCheckingEdgesShouldBeOnBoth() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(string.Empty);
        Select(input, 0, 0);

        Assert.Equal((true, true), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Theory]
    [InlineData("ab\rcd", 2, true, false)]
    [InlineData("ab\rcd", 3, false, true)]
    [InlineData("ab\r", 3, false, true)]
    public Task GivenLineBreakWhenCaretIsBesideItShouldBeOnTheEdgeOfItsLine(string text, int caret, bool up, bool down) => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(text);
        Select(input, caret, caret);

        Assert.Equal((up, down), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenLineBreaksWhenCaretIsOnTheMiddleLineShouldBeOnNeitherEdge() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync("first\rsecond\rthird");
        Select(input, 8, 8);
        Assert.Equal(1, CaretLine(input));

        Assert.Equal((false, false), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenSoftLineBreakWhenCaretIsAfterItShouldOnlyBeOnTheBottomEdge() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync("first\vsecond");
        Select(input, 8, 8);
        Assert.Equal(1, CaretLine(input));

        Assert.Equal((false, true), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    [Fact]
    public Task GivenUnfocusedInputWhenCheckingEdgesShouldBeOnBoth() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        var input = query.FindDescendant<RichEditBox>()!;
        query.Text = Paragraph;
        var caret = FirstPositionOnLine(input, 1) + 5;
        Select(input, caret, caret);
        Assert.NotSame(input, FocusManager.GetFocusedElement(input.XamlRoot));

        Assert.Equal((true, true), (query.CaretOnEdge(Up), query.CaretOnEdge(Down)));
    });

    private async Task<(QueryView Query, RichEditBox Input)> FocusedAsync(string text)
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        query.Text = text;
        query.FocusInput();
        await ui.IdleAsync();
        return (query, query.FindDescendant<RichEditBox>()!);
    }

    private static void Select(RichEditBox input, int start, int end)
    {
        input.Document.Selection.SetRange(start, end);
    }

    private static int CaretLine(RichEditBox input)
    {
        return LineOf(input, input.Document.Selection);
    }

    private static int FirstPositionOnLine(RichEditBox input, int line)
    {
        var position = 0;
        while (LineOf(input, input.Document.GetRange(position, position + 1)) < line)
        {
            position++;
        }

        return position;
    }

    private static int LineOf(RichEditBox input, ITextRange range)
    {
        input.Document.GetRange(0, 1).GetRect(PointOptions.ClientCoordinates, out var first, out _);
        range.GetRect(PointOptions.ClientCoordinates, out var rect, out _);
        return (int)Math.Round((rect.Top - first.Top) / first.Height);
    }
}
