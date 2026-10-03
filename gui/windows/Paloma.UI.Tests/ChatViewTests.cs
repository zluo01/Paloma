using CommunityToolkit.WinUI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.ViewModels.Overlay;
using Paloma.Views.Overlay.Chat;
using Paloma.UI.Tests.Helpers;
using Xunit;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed class ChatViewTests(UiFixture ui)
{
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public Task GivenAnotherProgramReadsTheClipboardOnChangeWhenCopyingTheSelectionShouldCopyIt(bool prompt) =>
        ui.RunAsync(async () =>
        {
            var (chat, _) = await SelectedAsync(prompt);
            await using var listener = await ClipboardListener.StartAsync();

            Assert.True(chat.CopySelection());

            Assert.Equal("hello world", await listener.Text);
        });

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public Task GivenSelectionLeftInUnfocusedTextWhenCopyingShouldCopyNothing(bool prompt) =>
        ui.RunAsync(async () =>
        {
            var (chat, input) = await SelectedAsync(prompt);
            input.Focus(FocusState.Keyboard);
            await ui.IdleAsync();

            Assert.False(chat.CopySelection());
        });

    private async Task<(ChatView Chat, TextBox Input)> SelectedAsync(bool prompt)
    {
        var chat = new ChatView { Width = 680, Height = 400 };
        chat.ViewModel.Sections.Add(prompt
            ? new UserSectionViewModel("hello world", [])
            : new ReasoningSectionViewModel("hello world") { IsExpanded = true });
        var input = new TextBox();
        var panel = new StackPanel();
        panel.Children.Add(chat);
        panel.Children.Add(input);
        await ui.ShowAsync(panel);
        chat.UpdateLayout();
        await ui.IdleAsync();
        foreach (var element in chat.FindDescendants())
        {
            switch (element)
            {
                case TextBlock { Text: "hello world" } block:
                    block.SelectAll();
                    block.Focus(FocusState.Keyboard);
                    break;
                case RichTextBlock block:
                    block.SelectAll();
                    block.Focus(FocusState.Keyboard);
                    break;
                default:
                    continue;
            }

            break;
        }

        await ui.IdleAsync();
        return (chat, input);
    }
}