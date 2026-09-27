using Windows.ApplicationModel.DataTransfer;
using Windows.Foundation;
using Windows.Graphics;
using Windows.System;
using CommunityToolkit.WinUI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Paloma.Helpers;
using Paloma.Models;
using Paloma.ViewModels.Overlay;
using DispatcherQueuePriority = Microsoft.UI.Dispatching.DispatcherQueuePriority;
using DispatcherQueueTimer = Microsoft.UI.Dispatching.DispatcherQueueTimer;
using Modifiers = Windows.Win32.UI.Input.KeyboardAndMouse.HOT_KEY_MODIFIERS;
using Behavior = PalomaCore.Behavior;

namespace Paloma.Views.Overlay;

public sealed partial class OverlayView
{
    private const int MoveDown = 1;
    private const int MoveUp = -1;

    public static readonly DependencyProperty ModeProperty = DependencyProperty.Register(
        nameof(Mode), typeof(OverlayMode), typeof(OverlayView), new PropertyMetadata(OverlayMode.Search));

    private static readonly TimeSpan SearchDebounce = TimeSpan.FromMilliseconds(150);

    private readonly DispatcherQueueTimer _inputDebounce;

    public OverlayViewModel ViewModel { get; }

    public SearchViewModel Search => SearchPanel.ViewModel;

    public SessionsViewModel Sessions => SessionsPanel.ViewModel;

    public OverlayMode Mode
    {
        get => (OverlayMode)GetValue(ModeProperty);
        private set => SetValue(ModeProperty, value);
    }

    public event Action? HideRequested;

    public event Action? ModeChanged;

    public event Action<RectInt32[], RectInt32[]>? DragRegionsChanged;

    public event Action? InputResized;

    private string InputText
    {
        get
        {
            Input.Document.GetText(TextGetOptions.None, out var text);
            return text.EndsWith('\r') ? text[..^1] : text;
        }
        set => Input.Document.SetText(TextSetOptions.None, value);
    }

    public double HeaderGrowth => Math.Max(0, Input.DesiredSize.Height - Input.MinHeight);

    public OverlayView()
    {
        ViewModel = new OverlayViewModel();
        InitializeComponent();
        _inputDebounce = DispatcherQueue.CreateTimer();
        foreach (var control in FooterPanel.InteractiveControls())
        {
            control.SizeChanged += OnSizeChanged;
        }
    }

    /// Only keyboard focus renders a caret. The queued retry covers
    /// a show that has not settled yet.
    public void FocusInput()
    {
        Input.Focus(FocusState.Keyboard);
        Input.Document.Selection.SetRange(int.MaxValue, int.MaxValue);
        DispatcherQueue.TryEnqueue(
            DispatcherQueuePriority.Low,
            () => Input.Focus(FocusState.Keyboard));
    }

    public static Visibility WhenSearch(OverlayMode mode)
    {
        return mode == OverlayMode.Search ? Visibility.Visible : Visibility.Collapsed;
    }

    public static Visibility WhenChat(OverlayMode mode)
    {
        return mode == OverlayMode.Chat ? Visibility.Visible : Visibility.Collapsed;
    }

    public static Visibility WhenSessions(OverlayMode mode)
    {
        return mode == OverlayMode.Sessions ? Visibility.Visible : Visibility.Collapsed;
    }

    private async void OnPreviewKeyDown(object sender, KeyRoutedEventArgs args)
    {
        var modifiers = Keyboard.GetPressedModifiers();

        // The mode's own bindings claim first; the globals catch the rest.
        switch (Mode)
        {
            case OverlayMode.Search:
                HandleSearchShortcut(args, modifiers);
                break;
            case OverlayMode.Chat:
                await HandleChatShortcutAsync(args, modifiers);
                break;
            case OverlayMode.Sessions:
                HandleSessionShortcut(args, modifiers);
                break;
        }

        if (args.Handled)
        {
            return;
        }

        switch (args.Key)
        {
            case VirtualKey.H when modifiers == Modifiers.MOD_CONTROL:
                args.Handled = true;
                if (Mode == OverlayMode.Sessions)
                {
                    Sessions.CancelPendingDelete();
                    InputText = string.Empty;
                    SetMode(OverlayMode.Search);
                }
                else
                {
                    await OpenSessionsAsync();
                }

                break;
            // A focused button owns Enter (tab reaches the chrome and the
            // decision buttons); everywhere else Enter is the submit.
            case VirtualKey.Enter
                when FocusManager.GetFocusedElement(XamlRoot)
                    is ButtonBase:
                break;
            case VirtualKey.Enter when modifiers == default:
                args.Handled = true;
                await SubmitAsync();
                break;
            case VirtualKey.Escape:
                args.Handled = true;
                if (Mode is OverlayMode.Sessions or OverlayMode.Chat)
                {
                    InputText = string.Empty;
                    SetMode(OverlayMode.Search);
                }
                else if (InputText.Length > 0)
                {
                    InputText = string.Empty;
                }
                else
                {
                    HideRequested?.Invoke();
                }

                break;
        }
    }

    private void HandleSearchShortcut(KeyRoutedEventArgs args, Modifiers modifiers)
    {
        switch (args.Key)
        {
            case VirtualKey.Up or VirtualKey.Down when modifiers == default && CaretOnEdge(args.Key):
                SearchPanel.Move(args.Key == VirtualKey.Down ? MoveDown : MoveUp);
                args.Handled = true;
                break;
            case VirtualKey.Enter when modifiers == Modifiers.MOD_CONTROL:
                SearchPanel.ShowActions();
                args.Handled = true;
                break;
        }
    }

    private async Task HandleChatShortcutAsync(KeyRoutedEventArgs args, Modifiers modifiers)
    {
        switch (args.Key)
        {
            case VirtualKey.Up or VirtualKey.Down when modifiers == default && CaretOnEdge(args.Key):
                Chat.Navigate(args.Key == VirtualKey.Down ? MoveDown : MoveUp);
                args.Handled = true;
                break;
            case VirtualKey.PageUp or VirtualKey.PageDown when modifiers == default:
                Chat.PageScroll(args.Key == VirtualKey.PageDown ? MoveDown : MoveUp);
                args.Handled = true;
                break;
            // Home/End is occupied by the input
            case VirtualKey.Home or VirtualKey.End when modifiers == Modifiers.MOD_CONTROL:
                Chat.EdgeScroll(args.Key == VirtualKey.End ? MoveDown : MoveUp);
                args.Handled = true;
                break;
            // Only a streaming turn claims Ctrl+C; otherwise it stays the
            // clipboard copy everyone expects in a text box.
            case VirtualKey.C when modifiers == Modifiers.MOD_CONTROL
                                   && Chat.ViewModel.Streaming:
                // select on the input prompt
                if (Input.Document.Selection.Length != 0)
                {
                    break;
                }

                args.Handled = true;
                // favor content copy over interrupt
                if (!Chat.CopySelection())
                {
                    await Chat.ViewModel.InterruptAsync();
                }

                break;
        }
    }

    private void HandleSessionShortcut(KeyRoutedEventArgs args, Modifiers modifiers)
    {
        switch (args.Key)
        {
            case VirtualKey.Up or VirtualKey.Down when modifiers == default && CaretOnEdge(args.Key):
                SessionsPanel.Move(args.Key == VirtualKey.Down ? MoveDown : MoveUp);
                args.Handled = true;
                break;
            case VirtualKey.Delete when modifiers == default:
                Sessions.PendingDelete();
                args.Handled = true;
                break;
            // if it is session mode and with delete signal on, cancel that first
            case VirtualKey.Escape when Sessions.CancelPendingDelete():
                args.Handled = true;
                break;
        }
    }

    // Check if the caret is either top or bottom
    // when in multiple line, we only move the result with arrow keys when caret is moved.
    private bool CaretOnEdge(VirtualKey key)
    {
        if (!ReferenceEquals(FocusManager.GetFocusedElement(XamlRoot), Input))
        {
            return true;
        }

        var delta = key == VirtualKey.Down ? MoveDown : MoveUp;
        var text = InputText;
        var selection = Input.Document.Selection;
        var caret = Math.Min(delta < 0 ? selection.StartPosition : selection.EndPosition, text.Length);
        return Composer.CaretOnEdge(delta, text, caret);
    }

    private async void OnInputPaste(object sender, TextControlPasteEventArgs args)
    {
        args.Handled = true;
        var content = Clipboard.GetContent();
        if (content.Contains(StandardDataFormats.StorageItems))
        {
            var items = await content.GetStorageItemsAsync();
            var paths = items
                .Select(item => item.Path)
                .Where(path => path.Length != 0)
                .Select(Composer.QuotePath)
                .ToList();
            if (paths.Count != 0)
            {
                InsertAtCaret(paths);
                return;
            }
        }

        if (content.Contains(StandardDataFormats.Text))
        {
            InsertAtCaret([await content.GetTextAsync()]);
        }
    }

    private void InsertAtCaret(List<string> parts)
    {
        var document = Input.Document;
        var selection = document.Selection;
        document.BeginUndoGroup();
        for (var i = 0; i < parts.Count; i++)
        {
            selection.SetText(TextSetOptions.None, i == 0 ? parts[i] : " " + parts[i]);
            selection.Collapse(false);
        }

        document.EndUndoGroup();
    }

    private void OnInputChanged(object sender, RoutedEventArgs args)
    {
        switch (Mode)
        {
            case OverlayMode.Search:
                _inputDebounce.Debounce(
                    () => _ = Search.SearchAsync(InputText),
                    SearchDebounce);
                break;
            case OverlayMode.Sessions:
                _inputDebounce.Debounce(
                    () => _ = Sessions.SearchAsync(InputText),
                    SearchDebounce);
                break;
        }
    }

    private async Task SubmitAsync()
    {
        switch (Mode)
        {
            case OverlayMode.Chat:
            {
                // handle the selected decision
                if (Chat.ViewModel.DecideSelected())
                {
                    return;
                }

                // submit prompt if not in streaming
                var prompt = InputText;
                if (!Chat.ViewModel.CanSubmit(prompt))
                {
                    return;
                }

                InputText = string.Empty;
                await Chat.ViewModel.SubmitAsync(prompt);
                return;
            }
            case OverlayMode.Sessions:
                if (await Sessions.ConfirmPendingDeleteAsync())
                {
                    return;
                }

                if (Sessions.Selected is { } sessionRow)
                {
                    RestoreSession(sessionRow);
                }

                return;
        }

        await SearchPanel.ActivateSelectedAsync();
    }

    private async Task StartChatAsync()
    {
        var prompt = InputText;
        if (!Chat.ViewModel.CanSubmit(prompt))
        {
            return;
        }

        InputText = string.Empty;
        SetMode(OverlayMode.Chat);
        await Chat.ViewModel.SubmitAsync(prompt);
    }

    private void RestoreSession(SessionRow row)
    {
        InputText = string.Empty;
        SetMode(OverlayMode.Chat);
        _ = Chat.ViewModel.RestoreAsync(row.Item.SessionId);
    }

    private async Task OpenSessionsAsync()
    {
        if (Mode == OverlayMode.Sessions)
        {
            return;
        }

        InputText = string.Empty;
        SetMode(OverlayMode.Sessions);
        await Sessions.LoadAsync();
        FocusInput();
    }

    private void SetMode(OverlayMode mode)
    {
        if (Mode == mode)
        {
            return;
        }

        // cancel current debounce before mode switch
        _inputDebounce.Stop();
        switch (Mode)
        {
            case OverlayMode.Search:
                Search.Clear();
                break;
            case OverlayMode.Chat:
                Chat.ViewModel.Clear();
                break;
        }

        Mode = mode;
        ModeChanged?.Invoke();
    }

    private void HandleBehavior(Behavior? behavior)
    {
        switch (behavior)
        {
            case Behavior.Hide:
                InputText = string.Empty;
                Search.Clear();
                HideRequested?.Invoke();
                break;
            case Behavior.Replace replace:
                InputText = replace.Input;
                Input.Document.Selection.SetRange(int.MaxValue, int.MaxValue);
                break;
        }
    }

    private void OnChatDecisionHandled(object? sender, EventArgs args)
    {
        FocusInput();
    }

    // After picking a model, focus is left on the dropdown button.
    // Move it back to the input so the user can keep typing.
    private void OnModelFlyoutClosed(object? sender, EventArgs args)
    {
        FocusInput();
    }

    private void OnSessionRowActivated(object? sender, SessionRow row)
    {
        RestoreSession(row);
    }

    private void OnFooterSessionsRequested(object? sender, EventArgs args)
    {
        _ = OpenSessionsAsync();
    }

    private void OnFooterStopRequested(object? sender, EventArgs args)
    {
        _ = Chat.ViewModel.InterruptAsync();
    }

    private void OnSearchActionCompleted(object? sender, Behavior? behavior)
    {
        HandleBehavior(behavior);
    }

    private async void OnSearchAskRequested(object? sender, EventArgs args)
    {
        await StartChatAsync();
    }

    // Closing the actions popup leaves focus on it, not the input.
    // Move it back so the user can keep typing.
    private void OnSearchActionsFlyoutClosed(object? sender, EventArgs args)
    {
        FocusInput();
    }

    private void OnLoaded(object sender, RoutedEventArgs args)
    {
        // A monitor change can swap the scale without resizing any element,
        // so the root change also republishes.
        if (XamlRoot is { } root)
        {
            root.Changed += OnXamlRootChanged;
        }

        ComputeDraggableArea();
    }

    private void OnXamlRootChanged(XamlRoot root, XamlRootChangedEventArgs args)
    {
        ComputeDraggableArea();
    }

    private void OnSizeChanged(object sender, SizeChangedEventArgs args)
    {
        ComputeDraggableArea();
    }

    private void OnInputSizeChanged(object sender, SizeChangedEventArgs args)
    {
        InputResized?.Invoke();
    }

    private void ComputeDraggableArea()
    {
        if (XamlRoot is null)
        {
            return;
        }

        DragRegionsChanged?.Invoke(
            [Bounds(HeaderBar), Bounds(FooterPanel)],
            [Bounds(Input), .. FooterPanel.InteractiveControls().Select(Bounds)]);
    }

    private RectInt32 Bounds(FrameworkElement element)
    {
        var scale = XamlRoot.RasterizationScale;
        var bounds = element.TransformToVisual(null).TransformBounds(
            new Rect(0, 0, element.ActualWidth, element.ActualHeight));
        return new RectInt32(
            (int)Math.Round(bounds.X * scale),
            (int)Math.Round(bounds.Y * scale),
            (int)Math.Round(bounds.Width * scale),
            (int)Math.Round(bounds.Height * scale));
    }
}