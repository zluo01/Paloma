using CommunityToolkit.WinUI;
using CommunityToolkit.WinUI.Controls;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Paloma.ViewModels.Overlay;
using Paloma.Views.Overlay.Chat;
using Xunit;
using UserPromptAttachment = PalomaCore.UserPromptAttachment;

namespace Paloma.UI.Tests;

[Collection(UiCollectionDefinition.Name)]
public sealed class PromptViewTests(UiFixture ui)
{
    [Fact]
    public Task GivenPromptWithAnImageWhenShownShouldShowItInlineAndAsAPreview() => ui.RunAsync(async () =>
    {
        var image = new UserPromptAttachment.Image(1, "image/png", [1, 2, 3]);
        var section = new UserSectionViewModel("look [Image #1] here", [image]);

        var view = await ui.ShowAsync(new PromptView { Section = section });

        var paragraph = (Paragraph)view.FindDescendant<RichTextBlock>()!.Blocks[0];
        Assert.Equal(3, paragraph.Inlines.Count);
        Assert.Equal("look ", ((Run)paragraph.Inlines[0]).Text);
        Assert.IsType<Image>(((InlineUIContainer)paragraph.Inlines[1]).Child);
        Assert.Equal(" here", ((Run)paragraph.Inlines[2]).Text);
        Assert.IsType<Image>(Assert.Single(view.FindDescendant<WrapPanel>()!.Children));
    });
}