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
}
