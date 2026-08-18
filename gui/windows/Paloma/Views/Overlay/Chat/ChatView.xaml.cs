using CommunityToolkit.WinUI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Paloma.Models;
using Paloma.ViewModels.Overlay;
using PermissionState = Paloma.Binding.V1.PermissionState;

namespace Paloma.Views.Overlay.Chat;

public sealed partial class ChatView
{
    private const double PageFraction = 0.9;

    // A new chat starts at the bottom: set while the sections are empty,
    // spent on the first scrollable layout. Anchoring keeps the view there.
    private bool _startAtBottom = true;

    public ChatViewModel ViewModel { get; }

    public event EventHandler? DecisionHandled;

    public ChatView()
    {
        ViewModel = new ChatViewModel(App.Current.Client);
        InitializeComponent();
    }

    public void Navigate(int delta)
    {
        if (ViewModel.Navigate(delta) is { } section
            && SectionsItems.ContainerFromItem(section) is FrameworkElement container)
        {
            container.StartBringIntoView();
        }
    }

    public void PageScroll(int direction)
    {
        ScrollBy(direction * SectionsScroller.ViewportHeight * PageFraction);
    }

    public void EdgeScroll(int direction)
    {
        SectionsScroller.ChangeView(
            null, direction < 0 ? 0 : SectionsScroller.ScrollableHeight, null, true);
    }

    public bool CopySelection()
    {
        switch (this.FindDescendants().FirstOrDefault(HasSelection))
        {
            case TextBlock block:
                block.CopySelectionToClipboard();
                return true;
            case RichTextBlock block:
                block.CopySelectionToClipboard();
                return true;
            default:
                return false;
        }
    }

    public static string ChevronGlyph(bool expanded)
    {
        return expanded ? "\uE70D" : "\uE76C";
    }

    public static string ResolutionLabel(PermissionState? resolution)
    {
        return resolution switch
        {
            PermissionState.Allow => "Allowed",
            PermissionState.Deny => "Denied",
            _ => "Error",
        };
    }

    public static Brush ResolutionBrush(PermissionState? resolution)
    {
        var key = resolution switch
        {
            PermissionState.Allow => "SystemFillColorSuccessBrush",
            PermissionState.Deny => "SystemFillColorCriticalBrush",
            _ => "SystemFillColorCautionBrush",
        };
        return (Brush)Application.Current.Resources[key];
    }

    public static Visibility WhenResolved(PermissionState? resolution)
    {
        return resolution is null ? Visibility.Collapsed : Visibility.Visible;
    }

    public static Visibility WhenCancelled(ChatStatus status)
    {
        return status == ChatStatus.Cancelled ? Visibility.Visible : Visibility.Collapsed;
    }

    private void OnSectionsSizeChanged(object sender, SizeChangedEventArgs args)
    {
        // On new conversation or restore, there is nothing to scroll with, height will be 0
        // hence, use this as proxy to re-request the one-time jump.
        if (SectionsScroller.ScrollableHeight == 0)
        {
            _startAtBottom = true;
            return;
        }

        if (!_startAtBottom) return;
        // Applied inside this layout pass, so the first presented frame
        // is already at the bottom.
        _startAtBottom = false;
        SectionsScroller.ChangeView(
            null, SectionsScroller.ScrollableHeight, null, true);
    }

    private void OnReasoningToggle(object sender, RoutedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext is ReasoningSectionViewModel section)
        {
            section.IsExpanded = !section.IsExpanded;
        }
    }

    private void OnToolDescriptionTapped(object sender, TappedRoutedEventArgs args)
    {
        if (sender is TextBlock description)
        {
            description.MaxLines = description.MaxLines == 2 ? 0 : 2;
        }
    }

    private void OnToolArgsTapped(object sender, TappedRoutedEventArgs args)
    {
        if (sender is TextBlock arguments)
        {
            arguments.MaxLines = arguments.MaxLines == 6 ? 0 : 6;
        }
    }

    private void OnDecisionClick(object sender, RoutedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext is not DecisionViewModel decision) return;
        decision.Decide();
        // Reset the focus back to the input on decide
        DecisionHandled?.Invoke(this, EventArgs.Empty);
    }

    private void ScrollBy(double delta)
    {
        SectionsScroller.ChangeView(
            null, SectionsScroller.VerticalOffset + delta, null, true);
    }

    private static bool HasSelection(DependencyObject element)
    {
        return element is TextBlock { SelectedText.Length: > 0 }
            or RichTextBlock { SelectedText.Length: > 0 };
    }
}