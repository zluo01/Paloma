using Windows.ApplicationModel.DataTransfer;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;
using Windows.System;
using CommunityToolkit.WinUI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Paloma.Models;
using Paloma.Views.Overlay.Query;
using Paloma.UI.Tests.Helpers;
using Xunit;
using UserPromptAttachment = PalomaCore.UserPromptAttachment;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed class QueryViewTests(UiFixture ui)
{
    private const int Up = -1;
    private const int Down = 1;
    private const VirtualKey OemPlus = (VirtualKey)187;
    private const VirtualKey OemComma = (VirtualKey)188;
    private const VirtualKey OemPeriod = (VirtualKey)190;

    // Wraps to four lines at the overlay width, with no line breaks.
    private static readonly string Paragraph = string.Join(" ", Enumerable.Repeat("one long paragraph that wraps", 8));

    private static readonly InlineImage Png = new("image/png", [1, 2, 3]);
    private static readonly InlineImage Jpeg = new("image/jpeg", [4, 5]);

    private static Task<(StorageFile Image, StorageFile Text)>? _pastableFiles;

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
    public Task GivenWrappedParagraphWhenCaretIsAtTheStartOfTheSecondLineShouldBeOnNeitherEdge() =>
        ui.RunAsync(async () =>
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
    public Task GivenLineBreakWhenCaretIsBesideItShouldBeOnTheEdgeOfItsLine(string text, int caret, bool up,
        bool down) => ui.RunAsync(async () =>
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

    [Fact]
    public Task GivenTextWithImagesWhenComposingShouldNumberThemInOrder() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });

        await AppendAsync(query, "look ", Png, " and ", Jpeg);

        var (prompt, attachments) = query.Compose();
        Assert.Equal("look [Image #1] and [Image #2]", prompt);
        AssertImages(attachments, (1, Png), (2, Jpeg));
    });

    [Fact]
    public Task GivenImagesPlacedOutOfInsertionOrderWhenComposingShouldNumberThemByPosition() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        await AppendAsync(query, " then ", Png);
        query.FindDescendant<RichEditBox>()!.Document.Selection.SetRange(0, 0);

        await AppendAsync(query, Jpeg);

        var (prompt, attachments) = query.Compose();
        Assert.Equal("[Image #1] then [Image #2]", prompt);
        AssertImages(attachments, (1, Jpeg), (2, Png));
    });

    [Fact]
    public Task GivenNoImagesWhenComposingShouldSendTheTextAlone() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });

        await AppendAsync(query, "hello");

        var (prompt, attachments) = query.Compose();
        Assert.Equal("hello", prompt);
        Assert.Empty(attachments);
    });

    [Fact]
    public Task GivenDeletedImageWhenComposingShouldLeaveItOut() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        await AppendAsync(query, Png, " and ", Jpeg);

        query.FindDescendant<RichEditBox>()!.Document.GetRange(0, 1).SetText(TextSetOptions.None, string.Empty);

        var (prompt, attachments) = query.Compose();
        Assert.Equal(" and [Image #1]", prompt);
        AssertImages(attachments, (1, Jpeg));
    });

    [Fact]
    public Task GivenSelectionWithoutImagesWhenCopyingShouldLeaveItToTheBuiltInCopy() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        await AppendAsync(query, "look ", Png, " and ", Jpeg, " end");

        Select(query.FindDescendant<RichEditBox>()!, 0, 4);

        Assert.Null(query.CopyText());
    });

    [Fact]
    public Task GivenWholePromptWhenCopyingShouldNumberImagesInOrder() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        await AppendAsync(query, "look ", Png, " and ", Jpeg, " end");

        Select(query.FindDescendant<RichEditBox>()!, 0, 16);

        Assert.Equal("look [Image #1] and [Image #2] end", query.CopyText());
    });

    [Fact]
    public Task GivenSecondImageWhenCopyingShouldKeepItsNumberInThePrompt() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        await AppendAsync(query, "look ", Png, " and ", Jpeg, " end");

        Select(query.FindDescendant<RichEditBox>()!, 11, 12);

        Assert.Equal("[Image #2]", query.CopyText());
    });

    [Fact]
    public Task GivenSelectionStartingAfterTheFirstImageWhenCopyingShouldKeepTheNumbersInThePrompt() =>
        ui.RunAsync(async () =>
        {
            var query = await ui.ShowAsync(new QueryView { Width = 680 });
            await AppendAsync(query, Png, " a ", Jpeg, " b ", Png);

            Select(query.FindDescendant<RichEditBox>()!, 1, 9);

            Assert.Equal(" a [Image #2] b [Image #3]", query.CopyText());
        });

    [Fact]
    public Task GivenLineBreaksWhenCopyingShouldUseCrlf() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        await AppendAsync(query, "x\r", Png, "\vy");

        Select(query.FindDescendant<RichEditBox>()!, 0, 5);

        Assert.Equal("x\r\n[Image #1]\r\ny", query.CopyText());
    });

    [Fact]
    public Task GivenSelectAllWhenCopyingShouldNotAddATrailingLineBreak() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        await AppendAsync(query, Png, " tail");

        Select(query.FindDescendant<RichEditBox>()!, 0, int.MaxValue);

        Assert.Equal("[Image #1] tail", query.CopyText());
    });

    [Fact]
    public Task GivenFocusedInputWhenPressingALetterShouldTypeIt() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(string.Empty);

        await ui.PressAsync(input, VirtualKey.A);

        Assert.Equal("a", query.Text);
    });

    [Theory]
    [InlineData(VirtualKey.B, false)]
    [InlineData(VirtualKey.I, false)]
    [InlineData(VirtualKey.U, false)]
    [InlineData(VirtualKey.E, false)]
    [InlineData(VirtualKey.R, false)]
    [InlineData(VirtualKey.J, false)]
    [InlineData(VirtualKey.Number1, false)]
    [InlineData(VirtualKey.Number2, false)]
    [InlineData(VirtualKey.Number5, false)]
    [InlineData(VirtualKey.L, true)]
    [InlineData(VirtualKey.A, true)]
    [InlineData(OemComma, true)]
    [InlineData(OemPeriod, true)]
    [InlineData(OemPlus, false)]
    [InlineData(OemPlus, true)]
    public Task GivenSelectedTextWhenPressingAFormattingShortcutShouldKeepItPlain(VirtualKey key, bool shift) =>
        ui.RunAsync(async () =>
        {
            var (_, input) = await FocusedAsync("hello world");
            Select(input, 0, 11);
            var plain = Format(input);

            await ui.PressAsync(input, key, shift ? [VirtualKey.Control, VirtualKey.Shift] : [VirtualKey.Control]);

            Assert.Equal(plain, Format(input));
        });

    [Fact]
    public Task GivenFormattingShortcutWhenClearedShouldTypeTheNextPromptPlain() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync("hello world");
        Select(input, 0, 11);
        var plain = Format(input);
        await ui.PressAsync(input, VirtualKey.A, VirtualKey.Control, VirtualKey.Shift);

        query.Clear();
        query.Text = "hello world";

        Assert.Equal(plain, Format(input));
    });

    [Fact]
    public Task GivenTextWhenPressingSelectAllShouldSelectAllOfIt() => ui.RunAsync(async () =>
    {
        var (_, input) = await FocusedAsync("hello world");
        Select(input, 0, 0);

        await ui.PressAsync(input, VirtualKey.A, VirtualKey.Control);

        input.Document.Selection.GetText(TextGetOptions.None, out var selected);
        Assert.Equal("hello world", selected.TrimEnd('\r'));
    });

    [Fact]
    public Task GivenTypedLetterWhenPressingUndoShouldRemoveIt() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(string.Empty);
        await ui.PressAsync(input, VirtualKey.A);

        await ui.PressAsync(input, VirtualKey.Z, VirtualKey.Control);

        Assert.Equal(string.Empty, query.Text);
    });

    [Fact]
    public Task GivenCaretAtTheEndWhenPressingWordLeftShouldMoveToTheLastWord() => ui.RunAsync(async () =>
    {
        var (_, input) = await FocusedAsync("hello world");

        await ui.PressAsync(input, VirtualKey.Left, VirtualKey.Control);

        Assert.Equal(6, input.Document.Selection.StartPosition);
    });

    [Fact]
    public Task GivenAnotherProgramReadsTheClipboardOnChangeWhenCopyingPlainTextShouldCopyIt() =>
        ui.RunAsync(async () =>
        {
            var (_, input) = await FocusedAsync("hello world");
            Select(input, 0, 11);
            await using var listener = await ClipboardListener.StartAsync();

            await ui.PressAsync(input, VirtualKey.C, VirtualKey.Control);

            Assert.Equal("hello world", await listener.Text);
        });

    [Fact]
    public Task GivenAnotherProgramReadsTheClipboardOnChangeWhenCopyingAnImageSelectionShouldCopyIt() =>
        ui.RunAsync(async () =>
        {
            var (query, input) = await FocusedAsync(string.Empty);
            await AppendAsync(query, "look ", Png);
            Select(input, 0, 6);
            await using var listener = await ClipboardListener.StartAsync();

            await ui.PressAsync(input, VirtualKey.C, VirtualKey.Control);

            Assert.Equal("look [Image #1]", await listener.Text);
        });

    [Fact]
    public Task GivenAnotherProgramReadsTheClipboardOnChangeWhenCuttingAnImageSelectionShouldCutIt() =>
        ui.RunAsync(async () =>
        {
            var (query, input) = await FocusedAsync(string.Empty);
            await AppendAsync(query, "look ", Png, " here");
            Select(input, 0, 6);
            await using var listener = await ClipboardListener.StartAsync();

            await ui.PressAsync(input, VirtualKey.X, VirtualKey.Control);

            Assert.Equal(("look [Image #1]", " here"), (await listener.Text, query.Text));
        });

    [Fact]
    public Task GivenSelectionLeftInUnfocusedInputWhenCheckingForASelectionShouldReportNone() => ui.RunAsync(async () =>
    {
        var query = new QueryView { Width = 680 };
        var other = new TextBox();
        var panel = new StackPanel();
        panel.Children.Add(query);
        panel.Children.Add(other);
        await ui.ShowAsync(panel);
        query.Text = "hello world";
        query.FocusInput();
        await ui.IdleAsync();
        Select(query.FindDescendant<RichEditBox>()!, 0, 5);

        other.Focus(FocusState.Keyboard);
        await ui.IdleAsync();

        Assert.False(query.HasSelection);
    });

    [Fact]
    public Task GivenTextOnTheClipboardWhenPastingShouldInsertItAtTheCaret() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync("ab");
        Select(input, 1, 1);
        var package = new DataPackage();
        package.SetText("XY");
        Clipboard.SetContent(package);

        await ui.PressAsync(input, VirtualKey.V, VirtualKey.Control);
        await PastedAsync(query, "aXYb");

        Assert.Equal(("aXYb", false), (query.Text, query.HasSelection));
    });

    [Fact]
    public Task GivenAnImageOnTheClipboardWhenPastingShouldInsertItFollowedByASpace() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(string.Empty);
        var package = new DataPackage();
        package.SetBitmap(RandomAccessStreamReference.CreateFromStream(await ThumbnailAsync()));
        Clipboard.SetContent(package);

        await ui.PressAsync(input, VirtualKey.V, VirtualKey.Control);
        await PastedAsync(query, "\uFFFC ");

        var (prompt, attachments) = query.Compose();
        Assert.Equal("[Image #1] ", prompt);
        Assert.Equal("image/png", Assert.IsType<UserPromptAttachment.Image>(Assert.Single(attachments)).MediaType);
    });

    [Fact]
    public Task GivenFilesOnTheClipboardWhenPastingShouldInsertImagesInlineAndOtherFilesAsQuotedPaths() =>
        ui.RunAsync(async () =>
        {
            var (query, input) = await FocusedAsync(string.Empty);
            var (image, text) = await PastableFilesAsync();
            var package = new DataPackage();
            package.SetStorageItems([image, text]);
            Clipboard.SetContent(package);

            await ui.PressAsync(input, VirtualKey.V, VirtualKey.Control);
            await PastedAsync(query, $"\uFFFC \"{text.Path}\" ");

            var (prompt, attachments) = query.Compose();
            Assert.Equal($"[Image #1] \"{text.Path}\" ", prompt);
            Assert.Single(attachments);
        });

    [Fact]
    public Task GivenAPasteOfSeveralItemsWhenUndoingShouldRemoveThemTogether() => ui.RunAsync(async () =>
    {
        var (query, input) = await FocusedAsync(string.Empty);
        var (image, text) = await PastableFilesAsync();
        var package = new DataPackage();
        package.SetStorageItems([image, text]);
        Clipboard.SetContent(package);
        await ui.PressAsync(input, VirtualKey.V, VirtualKey.Control);
        await PastedAsync(query, $"\uFFFC \"{text.Path}\" ");

        await ui.PressAsync(input, VirtualKey.Z, VirtualKey.Control);

        Assert.Equal(string.Empty, query.Text);
    });

    [Fact]
    public Task GivenMoreLinesWhenMeasuringShouldGrowAndRaiseInputResized() => ui.RunAsync(async () =>
    {
        var query = await ui.ShowAsync(new QueryView { Width = 680 });
        var input = query.FindDescendant<RichEditBox>()!;
        var singleLine = query.Growth;
        var resized = 0;
        query.InputResized += (_, _) => resized++;

        query.Text = "one\rtwo\rthree";
        query.UpdateLayout();
        await ui.IdleAsync();

        Assert.True(query.Growth > singleLine);
        Assert.Equal((input.DesiredSize.Height - input.MinHeight, 1), (query.Growth, resized));
    });

    private static async Task AppendAsync(QueryView query, params object[] parts)
    {
        var selection = query.FindDescendant<RichEditBox>()!.Document.Selection;
        foreach (var part in parts)
        {
            if (part is InlineImage image)
            {
                var caret = selection.StartPosition;
                using var thumbnail = await ThumbnailAsync();
                selection.InsertImage(16, 16, 0, VerticalCharacterAlignment.Bottom, query.ViewModel.Add(image),
                    thumbnail);
                selection.SetRange(caret + 1, caret + 1);
            }
            else
            {
                selection.SetText(TextSetOptions.None, (string)part);
                selection.Collapse(false);
            }
        }
    }

    private async Task PastedAsync(QueryView query, string text)
    {
        for (var attempt = 0; attempt < 50 && query.Text != text; attempt++)
        {
            await Task.Delay(20);
            await ui.IdleAsync();
        }
    }

    private static Task<(StorageFile Image, StorageFile Text)> PastableFilesAsync()
    {
        return _pastableFiles ??= CreatePastableFilesAsync();
    }

    private static async Task<(StorageFile Image, StorageFile Text)> CreatePastableFilesAsync()
    {
        var folder = await StorageFolder.GetFolderFromPathAsync(AppContext.BaseDirectory);
        var image = await folder.CreateFileAsync("pasted.png", CreationCollisionOption.ReplaceExisting);
        using (var png = await ThumbnailAsync())
        using (var target = await image.OpenAsync(FileAccessMode.ReadWrite))
        {
            await RandomAccessStream.CopyAsync(png, target);
        }

        var text = await folder.CreateFileAsync("pasted.txt", CreationCollisionOption.ReplaceExisting);
        await FileIO.WriteTextAsync(text, "not an image");
        return (image, text);
    }

    private static async Task<InMemoryRandomAccessStream> ThumbnailAsync()
    {
        var stream = new InMemoryRandomAccessStream();
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, 1, 1, 96, 96, [0, 0, 0, 255]);
        await encoder.FlushAsync();
        stream.Seek(0);
        return stream;
    }

    private static void AssertImages(UserPromptAttachment[] attachments, params (uint Id, InlineImage Image)[] expected)
    {
        Assert.Equal(expected.Length, attachments.Length);
        foreach (var ((id, image), attachment) in expected.Zip(attachments))
        {
            var sent = Assert.IsType<UserPromptAttachment.Image>(attachment);
            Assert.Equal(id, sent.Id);
            Assert.Equal(image.MediaType, sent.MediaType);
            Assert.Same(image.Data, sent.Data);
        }
    }

    private static string Format(RichEditBox input)
    {
        var range = input.Document.GetRange(0, 5);
        var paragraph = range.ParagraphFormat;
        var character = range.CharacterFormat;
        return $"align={paragraph.Alignment} list={paragraph.ListType} spacing={paragraph.LineSpacingRule} " +
               $"size={character.Size} bold={character.Bold} italic={character.Italic} underline={character.Underline} " +
               $"caps={character.AllCaps} sub={character.Subscript} super={character.Superscript}";
    }

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