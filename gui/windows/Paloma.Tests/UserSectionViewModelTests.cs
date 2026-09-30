using Paloma.ViewModels.Overlay;
using Xunit;
using UserPromptAttachment = PalomaCore.UserPromptAttachment;

namespace Paloma.Tests;

public sealed class UserSectionViewModelTests
{
    private static readonly byte[] First = [1, 2, 3];
    private static readonly byte[] Second = [4, 5];

    private static readonly UserPromptAttachment[] Attachments =
    [
        new UserPromptAttachment.Image(1, "image/png", First),
        new UserPromptAttachment.Image(2, "image/jpeg", Second),
    ];

    private static string Joined(UserSectionViewModel section)
    {
        return string.Concat(section.Segments.Select(segment => ((UserSectionViewModel.Segment.Text)segment).Value));
    }

    [Fact]
    public void GivenPlaceholdersWhenBuildingSectionShouldSplitTextAndImagesInOrder()
    {
        var section = new UserSectionViewModel("compare [Image #1] with [Image #2], ok?", Attachments);

        Assert.Equal(5, section.Segments.Count);
        Assert.Equal("compare ", Assert.IsType<UserSectionViewModel.Segment.Text>(section.Segments[0]).Value);
        Assert.Same(First, Assert.IsType<UserSectionViewModel.Segment.Image>(section.Segments[1]).Data);
        Assert.Equal(" with ", Assert.IsType<UserSectionViewModel.Segment.Text>(section.Segments[2]).Value);
        Assert.Same(Second, Assert.IsType<UserSectionViewModel.Segment.Image>(section.Segments[3]).Data);
        Assert.Equal(", ok?", Assert.IsType<UserSectionViewModel.Segment.Text>(section.Segments[4]).Value);
    }

    [Fact]
    public void GivenPlaceholdersAtTheEdgesWhenBuildingSectionShouldNotAddEmptyText()
    {
        var section = new UserSectionViewModel("[Image #1] hi [Image #2]", Attachments);

        Assert.Equal(3, section.Segments.Count);
        Assert.Same(First, Assert.IsType<UserSectionViewModel.Segment.Image>(section.Segments[0]).Data);
        Assert.Equal(" hi ", Assert.IsType<UserSectionViewModel.Segment.Text>(section.Segments[1]).Value);
        Assert.Same(Second, Assert.IsType<UserSectionViewModel.Segment.Image>(section.Segments[2]).Data);
    }

    [Theory]
    [InlineData("see [Image #7]")]
    [InlineData("see [Image #x] and [Image #")]
    [InlineData("plain text")]
    [InlineData("")]
    public void GivenNoMatchingPlaceholderWhenBuildingSectionShouldKeepEverythingAsText(string text)
    {
        var section = new UserSectionViewModel(text, Attachments);

        Assert.All(section.Segments, segment => Assert.IsType<UserSectionViewModel.Segment.Text>(segment));
        Assert.Equal(text, Joined(section));
        Assert.Empty(section.Images);
    }

    [Fact]
    public void GivenPlaceholdersWhenBuildingSectionShouldListPreviewsInTextOrder()
    {
        var section = new UserSectionViewModel("[Image #2] then [Image #1]", Attachments);

        Assert.Equal(2, section.Images.Count);
        Assert.Same(Second, section.Images[0]);
        Assert.Same(First, section.Images[1]);
    }
}