using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Paloma.Helpers;
using Paloma.ViewModels.Overlay;
using Behavior = PalomaCore.Behavior;

namespace Paloma.Views.Overlay.Search;

public sealed partial class SearchView
{
    public SearchViewModel ViewModel { get; }

    public event EventHandler<Behavior?>? ActionCompleted;

    public event EventHandler? AskRequested;

    public event EventHandler? ActionsFlyoutClosed;

    public SearchView()
    {
        ViewModel = new SearchViewModel(App.Current.Client);
        InitializeComponent();
    }

    public async Task ActivateSelectedAsync()
    {
        if (ViewModel.SelectedRow is { } row)
        {
            if (row.PrimaryAction is { } action)
            {
                ActionCompleted?.Invoke(this, await ViewModel.ActivateAsync(row, action));
            }

            return;
        }

        AskRequested?.Invoke(this, EventArgs.Empty);
    }

    public void ShowActions()
    {
        if (ViewModel.SelectedRow is not { HasActionMenu: true } row)
        {
            return;
        }

        BuildActions(row);
        ActionsFlyout.ShowAt(ContainerFor(row) ?? this);
    }

    public void Move(int delta)
    {
        var index = ViewModel.Move(delta);
        if (ViewModel.AskSelected)
        {
            ResultsScroller.ChangeView(null, ResultsScroller.ScrollableHeight, null, disableAnimation: true);
            return;
        }

        if (ViewModel.SelectedRow is not { } row)
        {
            return;
        }

        RowScroller.BringIntoView(ResultsScroller, ContainerFor(row), index, ViewModel.RowCount);
    }

    private async void OnRowTapped(object sender, TappedRoutedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext
            is not LauncherRow { PrimaryAction: { } action } row)
        {
            return;
        }

        ActionCompleted?.Invoke(this, await ViewModel.ActivateAsync(row, action));
    }

    private void OnRowRightTapped(object sender, RightTappedRoutedEventArgs args)
    {
        if (sender is not FrameworkElement { DataContext: LauncherRow { HasActionMenu: true } row } element)
        {
            return;
        }

        // Right-click acts on the row under the cursor, like the shell:
        // select it and open the menu at the pointer.
        ViewModel.Select(row);
        BuildActions(row);
        ActionsFlyout.ShowAt(element, new FlyoutShowOptions { Position = args.GetPosition(element) });
    }

    private void OnRowMoreClick(object sender, RoutedEventArgs args)
    {
        if (sender is not FrameworkElement { DataContext: LauncherRow row } element)
        {
            return;
        }

        ViewModel.Select(row);
        BuildActions(row);
        ActionsFlyout.ShowAt(element);
    }

    // Stops the more click from bubbling into the row's Tapped and
    // running the primary action.
    private void OnRowMoreTapped(object sender, TappedRoutedEventArgs args)
    {
        args.Handled = true;
    }

    private void OnAskTapped(object sender, TappedRoutedEventArgs args)
    {
        AskRequested?.Invoke(this, EventArgs.Empty);
    }

    private void OnActionsFlyoutClosed(object sender, object args)
    {
        ActionsFlyoutClosed?.Invoke(this, EventArgs.Empty);
    }

    private void BuildActions(LauncherRow row)
    {
        ActionsFlyout.Items.Clear();
        foreach (var action in row.Item.Actions)
        {
            var item = new MenuFlyoutItem
            {
                Text = action.Label,
                Style = (Style)Resources["ActionMenuItem"],
            };
            item.Click += async (_, _) =>
                ActionCompleted?.Invoke(this, await ViewModel.ActivateAsync(row, action));
            ActionsFlyout.Items.Add(item);
        }
    }

    private FrameworkElement? ContainerFor(LauncherRow row)
    {
        foreach (var group in ViewModel.Groups)
        {
            if (!group.Items.Contains(row))
            {
                continue;
            }

            if (GroupsItems.ContainerFromItem(group) is not { } groupContainer)
            {
                return null;
            }

            return ItemsControlOf(groupContainer)?.ContainerFromItem(row) as FrameworkElement;
        }

        return null;
    }

    private static ItemsControl? ItemsControlOf(DependencyObject container)
    {
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(container); i++)
        {
            var child = VisualTreeHelper.GetChild(container, i);
            if (child is ItemsControl items)
            {
                return items;
            }

            if (ItemsControlOf(child) is { } nested)
            {
                return nested;
            }
        }

        return null;
    }
}