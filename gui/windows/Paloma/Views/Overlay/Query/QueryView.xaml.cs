using Windows.ApplicationModel.DataTransfer;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Paloma.Helpers;
using Paloma.Models;
using Paloma.ViewModels.Overlay;
using DispatcherQueuePriority = Microsoft.UI.Dispatching.DispatcherQueuePriority;

namespace Paloma.Views.Overlay.Query;

public sealed partial class QueryView
{
    // requires to listen on mode change
    public static readonly DependencyProperty ModeProperty = DependencyProperty.Register(
        nameof(Mode), typeof(OverlayMode), typeof(QueryView), new PropertyMetadata(OverlayMode.Search));

    public OverlayMode Mode
    {
        get => (OverlayMode)GetValue(ModeProperty);
        set => SetValue(ModeProperty, value);
    }

    public event EventHandler? TextChanged;

    public event EventHandler? InputResized;

    public double Growth => Math.Max(0, Input.DesiredSize.Height - Input.MinHeight);

    public bool HasSelection => Input.Document.Selection.Length != 0;

    // Text set from outside goes into the box with the caret at the end.
    public string Text
    {
        get
        {
            Input.Document.GetText(TextGetOptions.None, out var text);
            if (text.EndsWith('\r'))
            {
                return text[..^1];
            }

            return text;
        }
        set
        {
            Input.Document.SetText(TextSetOptions.None, value);
            Input.Document.Selection.SetRange(int.MaxValue, int.MaxValue);
        }
    }

    public QueryView()
    {
        InitializeComponent();
    }

    public void Clear()
    {
        Text = string.Empty;
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

    // The input is only asked when it holds the focus.
    public bool CaretOnEdge(int delta)
    {
        if (!ReferenceEquals(FocusManager.GetFocusedElement(XamlRoot), Input))
        {
            return true;
        }

        var selection = Input.Document.Selection;
        if (delta < 0)
        {
            return QueryViewModel.CaretOnEdge(delta, Text, selection.StartPosition);
        }

        return QueryViewModel.CaretOnEdge(delta, Text, selection.EndPosition);
    }

    // Exclude the input from the dragging area
    internal IReadOnlyList<FrameworkElement> InteractiveControls()
    {
        return [Input];
    }

    private void OnInputChanged(object sender, RoutedEventArgs args)
    {
        TextChanged?.Invoke(this, EventArgs.Empty);
    }

    private void OnInputSizeChanged(object sender, SizeChangedEventArgs args)
    {
        InputResized?.Invoke(this, EventArgs.Empty);
    }

    private async void OnInputPaste(object sender, TextControlPasteEventArgs args)
    {
        args.Handled = true;
        await InsertContentAsync(Clipboard.GetContent());
    }

    private void OnInputDragOver(object sender, DragEventArgs args)
    {
        args.Handled = true;
        if (args.DataView.Contains(StandardDataFormats.StorageItems)
            || args.DataView.Contains(StandardDataFormats.Text))
        {
            args.AcceptedOperation = DataPackageOperation.Copy;
        }
    }

    private async void OnInputDrop(object sender, DragEventArgs args)
    {
        args.Handled = true;
        var deferral = args.GetDeferral();
        try
        {
            var point = args.GetPosition(Input);
            var caret = Input.Document.GetRangeFromPoint(point, PointOptions.ClientCoordinates).StartPosition;
            Input.Document.Selection.SetRange(caret, caret);
            await InsertContentAsync(args.DataView);
        }
        finally
        {
            deferral.Complete();
            Input.Focus(FocusState.Keyboard);
        }
    }

    private async Task InsertContentAsync(DataPackageView content)
    {
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
}
