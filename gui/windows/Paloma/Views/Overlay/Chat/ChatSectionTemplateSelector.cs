using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.ViewModels.Overlay;

namespace Paloma.Views.Overlay.Chat;

public sealed partial class ChatSectionTemplateSelector : DataTemplateSelector
{
    public DataTemplate? User { get; set; }

    public DataTemplate? Assistant { get; set; }

    public DataTemplate? Reasoning { get; set; }

    public DataTemplate? Tool { get; set; }

    protected override DataTemplate? SelectTemplateCore(object item)
    {
        return item switch
        {
            UserSectionViewModel => User,
            AssistantSectionViewModel => Assistant,
            ReasoningSectionViewModel => Reasoning,
            ToolSectionViewModel => Tool,
            _ => null,
        };
    }

    protected override DataTemplate? SelectTemplateCore(object item, DependencyObject container)
    {
        return SelectTemplateCore(item);
    }
}