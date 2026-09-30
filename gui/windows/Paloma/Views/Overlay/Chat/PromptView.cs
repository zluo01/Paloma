using CommunityToolkit.WinUI.Controls;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Paloma.Helpers;
using Paloma.ViewModels.Overlay;

namespace Paloma.Views.Overlay.Chat;

public sealed partial class PromptView : ContentControl
{
    private const double InlineHeight = 16;
    private const double PreviewHeight = 120;

    public static readonly DependencyProperty SectionProperty = DependencyProperty.Register(
        nameof(Section),
        typeof(UserSectionViewModel),
        typeof(PromptView),
        new PropertyMetadata(null, (control, _) => ((PromptView)control).Rebuild()));

    public UserSectionViewModel? Section
    {
        get => (UserSectionViewModel?)GetValue(SectionProperty);
        set => SetValue(SectionProperty, value);
    }

    public PromptView()
    {
        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        IsTabStop = false;
    }

    private void Rebuild()
    {
        if (Section == null)
        {
            Content = null;
            return;
        }

        var paragraph = new Paragraph();
        foreach (var segment in Section.Segments)
        {
            switch (segment)
            {
                case UserSectionViewModel.Segment.Text text:
                    paragraph.Inlines.Add(new Run { Text = text.Value });
                    break;
                case UserSectionViewModel.Segment.Image image:
                    paragraph.Inlines.Add(new InlineUIContainer { Child = Images.Picture(image.Data, InlineHeight) });
                    break;
            }
        }

        var block = new RichTextBlock { TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true };
        block.Blocks.Add(paragraph);
        var panel = new StackPanel { Spacing = 6 };
        panel.Children.Add(block);
        if (Section.Images.Count > 0)
        {
            var previews = new WrapPanel { HorizontalSpacing = 6, VerticalSpacing = 6 };
            foreach (var data in Section.Images)
            {
                previews.Children.Add(Images.Picture(data, PreviewHeight));
            }

            panel.Children.Add(previews);
        }

        Content = panel;
    }
}
