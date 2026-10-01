using Windows.ApplicationModel.DataTransfer;
using Windows.Foundation;
using Windows.Storage;
using CommunityToolkit.WinUI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Paloma.Helpers;
using Paloma.Models;
using Paloma.ViewModels.Overlay;
using UserPromptAttachment = PalomaCore.UserPromptAttachment;
using DispatcherQueuePriority = Microsoft.UI.Dispatching.DispatcherQueuePriority;

namespace Paloma.Views.Overlay.Query;

public sealed partial class QueryView
{
    public static readonly DependencyProperty ModeProperty = DependencyProperty.Register(
        nameof(Mode), typeof(OverlayMode), typeof(QueryView), new PropertyMetadata(OverlayMode.Search));

    private const double PreviewHeight = 240;

    private string? _hovered;

    public QueryViewModel ViewModel { get; } = new();

    private UIElement DocumentContent => (UIElement)Input.FindDescendant<ScrollViewer>()!.Content;

    public OverlayMode Mode
    {
        get => (OverlayMode)GetValue(ModeProperty);
        set => SetValue(ModeProperty, value);
    }

    public event EventHandler? TextChanged;

    public event EventHandler? InputResized;

    public double Growth => Math.Max(0, Input.DesiredSize.Height - Input.MinHeight);

    public bool HasSelection => Input.Document.Selection.Length != 0;

    public string Text
    {
        get
        {
            Input.Document.GetText(TextGetOptions.None, out var text);
            return text.EndsWith('\r') ? text[..^1] : text;
        }
        set
        {
            Input.Document.SetText(TextSetOptions.None, value);
            Input.Document.Selection.SetRange(int.MaxValue, int.MaxValue);
        }
    }

    public string Prompt => Composer.Prompt(Text);

    public QueryView()
    {
        InitializeComponent();
        Input.AddHandler(PointerMovedEvent, new PointerEventHandler(OnInputPointerMoved), true);
    }

    public UserPromptAttachment[] Attachments()
    {
        var document = Input.Document;
        document.GetText(TextGetOptions.None, out var text);
        var keys = new List<string>();
        for (var i = 0; i < text.Length; i++)
        {
            if (text[i] != Images.ImageCharacter)
            {
                continue;
            }

            document.GetRange(i, i + 1).GetText(TextGetOptions.UseObjectText, out var key);
            keys.Add(key);
        }

        return ViewModel.Attachments(keys);
    }

    public void Clear()
    {
        Text = string.Empty;
        Input.Document.ClearUndoRedoHistory();
        ViewModel.Clear();
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

    public bool CaretOnEdge(int delta)
    {
        if (!ReferenceEquals(FocusManager.GetFocusedElement(XamlRoot), Input))
        {
            return true;
        }

        var selection = Input.Document.Selection;
        var caret = delta < 0 ? selection.StartPosition : selection.EndPosition;
        return Composer.CaretOnEdge(delta, Text, caret);
    }

    // Exclude the input from the dragging area
    internal IReadOnlyList<FrameworkElement> InteractiveControls()
    {
        return [Input];
    }

    private double LineHeight()
    {
        Input.Document.GetRange(0, 0).GetRect(PointOptions.ClientCoordinates, out var rect, out _);
        return rect.Height > 0 ? rect.Height : Input.FontSize;
    }

    private void OnInputPointerMoved(object sender, PointerRoutedEventArgs args)
    {
        var content = DocumentContent;
        var point = args.GetCurrentPoint(content).Position;
        var caret = Input.Document.GetRangeFromPoint(point, PointOptions.ClientCoordinates).StartPosition;
        foreach (var start in new[] { caret - 1, caret })
        {
            var range = Input.Document.GetRange(start, start + 1);
            range.GetRect(PointOptions.ClientCoordinates, out var rect, out _);
            if (range.Character != Images.ImageCharacter || !rect.Contains(point))
            {
                continue;
            }

            range.GetText(TextGetOptions.UseObjectText, out var key);
            if (key == _hovered)
            {
                return;
            }

            if (!ViewModel.TryGetImage(key, out var image))
            {
                continue;
            }

            _hovered = key;
            var anchor = content.TransformToVisual(Input).TransformBounds(rect);
            Preview.Hide();
            Preview.Content = Images.Picture(image.Data, PreviewHeight);
            Preview.ShowAt(Input, new FlyoutShowOptions
            {
                Position = new Point(anchor.X + anchor.Width / 2, anchor.Bottom),
                ExclusionRect = anchor,
                ShowMode = FlyoutShowMode.Transient,
            });
            return;
        }

        _hovered = null;
        Preview.Hide();
    }

    private void OnInputPointerExited(object sender, PointerRoutedEventArgs args)
    {
        _hovered = null;
        Preview.Hide();
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
            || args.DataView.Contains(StandardDataFormats.Text)
            || args.DataView.Contains(StandardDataFormats.Bitmap))
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
            var point = args.GetPosition(DocumentContent);
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
            var segments = new List<Segment>();
            foreach (var item in await content.GetStorageItemsAsync())
            {
                if (item is StorageFile file)
                {
                    var loaded = await Images.LoadFileAsync(file, LineHeight(), XamlRoot.RasterizationScale);
                    if (loaded != null)
                    {
                        segments.Add(new Segment.Image(loaded));
                        continue;
                    }
                }

                if (item.Path.Length != 0)
                {
                    segments.Add(new Segment.Text(Composer.QuotePath(item.Path)));
                }
            }

            if (segments.Count != 0)
            {
                InsertAtCaret(segments, true);
                return;
            }
        }

        if (content.Contains(StandardDataFormats.Text))
        {
            InsertAtCaret([new Segment.Text(await content.GetTextAsync())], false);
            return;
        }

        if (content.Contains(StandardDataFormats.Bitmap))
        {
            var loaded = await Images.LoadBitmapAsync(await content.GetBitmapAsync(), LineHeight(), XamlRoot.RasterizationScale);
            if (loaded != null)
            {
                InsertAtCaret([new Segment.Image(loaded)], true);
            }
        }
    }

    private void InsertAtCaret(List<Segment> segments, bool spaceAfterEach)
    {
        var document = Input.Document;
        var selection = document.Selection;
        document.BeginUndoGroup();
        foreach (var segment in segments)
        {
            switch (segment)
            {
                case Segment.Image image:
                    InsertImage(image.Loaded);
                    break;
                case Segment.Text text:
                    selection.SetText(TextSetOptions.None, text.Value);
                    selection.Collapse(false);
                    break;
            }

            if (!spaceAfterEach) continue;
            selection.SetText(TextSetOptions.None, " ");
            selection.Collapse(false);
        }

        document.EndUndoGroup();
    }

    private void InsertImage(Images.Loaded loaded)
    {
        var selection = Input.Document.Selection;
        var caret = selection.StartPosition;
        using (loaded.Thumbnail)
        {
            selection.InsertImage(
                loaded.Width,
                loaded.Height,
                0,
                VerticalCharacterAlignment.Bottom,
                ViewModel.Add(loaded.Image),
                loaded.Thumbnail);
        }

        selection.SetRange(caret + 1, caret + 1);
    }

    private void OnCopyingToClipboard(RichEditBox sender, TextControlCopyingToClipboardEventArgs args)
    {
        args.Handled = Copy();
    }

    private void OnCuttingToClipboard(RichEditBox sender, TextControlCuttingToClipboardEventArgs args)
    {
        args.Handled = Copy();
        if (args.Handled)
        {
            Input.Document.Selection.SetText(TextSetOptions.None, string.Empty);
        }
    }

    private bool Copy()
    {
        // fallback to built-in when no image
        var selection = Input.Document.Selection;
        selection.GetText(TextGetOptions.None, out var text);
        if (!text.Contains(Images.ImageCharacter, StringComparison.Ordinal))
        {
            return false;
        }

        if (text.EndsWith('\r')
            && selection.EndPosition >= Input.Document.GetRange(0, int.MaxValue).EndPosition)
        {
            text = text[..^1];
        }

        var package = new DataPackage();
        package.SetText(Composer.CopyText(text));
        Clipboard.SetContent(package);
        return true;
    }

    private abstract record Segment
    {
        public sealed record Text(string Value) : Segment;

        public sealed record Image(Images.Loaded Loaded) : Segment;
    }
}
