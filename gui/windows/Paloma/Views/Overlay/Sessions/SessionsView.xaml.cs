using CommunityToolkit.Mvvm.Messaging;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Paloma.Helpers;
using Paloma.Messages;
using Paloma.ViewModels.Overlay;

namespace Paloma.Views.Overlay.Sessions;

public sealed partial class SessionsView
{
    public SessionsViewModel ViewModel { get; }

    public event EventHandler<SessionRow>? RowActivated;

    public SessionsView()
    {
        ViewModel = new SessionsViewModel(App.Current.Client);
        InitializeComponent();
        // reset the pending delete status on hide
        WeakReferenceMessenger.Default.Register<OverlayHiddenMessage>(
            this, (_, _) => ViewModel.CancelPendingDelete());
    }

    public void Move(int delta)
    {
        var index = ViewModel.Move(delta);
        if (ViewModel.Selected is not { } row) return;
        RowScroller.BringIntoView(
            SessionsScroller, SessionsItems.ContainerFromItem(row) as FrameworkElement,
            index, ViewModel.Rows.Count);
    }

    private void OnSessionTapped(object sender, TappedRoutedEventArgs args)
    {
        if (sender is FrameworkElement { DataContext: SessionRow row })
        {
            RowActivated?.Invoke(this, row);
        }
    }

    private async void OnSessionDeleteClick(object sender, RoutedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext is not SessionRow row)
        {
            return;
        }

        // A click walks the same two-step confirmation as Del and Enter.
        if (row.PendingDeletion)
        {
            await ViewModel.ConfirmPendingDeleteAsync();
        }
        else
        {
            ViewModel.PendingDelete(row);
        }
    }

    public static Visibility WhenAny(bool hovered, bool pending)
    {
        return hovered || pending ? Visibility.Visible : Visibility.Collapsed;
    }

    public static Brush DeleteTint(bool pending)
    {
        var key = pending ? "SystemFillColorCriticalBrush" : "TextFillColorPrimaryBrush";
        return (Brush)Application.Current.Resources[key];
    }

    // Stops a delete click from bubbling into the row's Tapped and
    // triggering a restore.
    private void OnSessionDeleteTapped(object sender, TappedRoutedEventArgs args)
    {
        args.Handled = true;
    }
}