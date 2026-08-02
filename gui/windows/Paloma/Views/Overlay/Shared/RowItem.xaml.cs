using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Markup;

namespace Paloma.Views.Overlay.Shared;

/// <summary>
/// Draws the selected background, accent bar, and hover highlight around
/// the content a template places in Row. The overlay's list templates
/// have no item container of their own to draw these.
/// </summary>
[ContentProperty(Name = nameof(Row))]
public sealed partial class RowItem
{
    public static readonly DependencyProperty SelectedProperty = DependencyProperty.Register(
        nameof(Selected), typeof(bool), typeof(RowItem), new PropertyMetadata(false));

    public static readonly DependencyProperty HoveredProperty = DependencyProperty.Register(
        nameof(Hovered), typeof(bool), typeof(RowItem), new PropertyMetadata(false));

    public static readonly DependencyProperty RowProperty = DependencyProperty.Register(
        nameof(Row), typeof(UIElement), typeof(RowItem), new PropertyMetadata(null));

    public bool Selected
    {
        get => (bool)GetValue(SelectedProperty);
        set => SetValue(SelectedProperty, value);
    }

    public bool Hovered
    {
        get => (bool)GetValue(HoveredProperty);
        set => SetValue(HoveredProperty, value);
    }

    public UIElement? Row
    {
        get => (UIElement?)GetValue(RowProperty);
        set => SetValue(RowProperty, value);
    }

    public RowItem()
    {
        InitializeComponent();
    }

    private void OnPointerEntered(object sender, PointerRoutedEventArgs args)
    {
        Hovered = true;
    }

    private void OnPointerExited(object sender, PointerRoutedEventArgs args)
    {
        Hovered = false;
    }
}