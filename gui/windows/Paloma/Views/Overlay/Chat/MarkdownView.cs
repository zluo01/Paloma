using CommunityToolkit.Mvvm.Messaging;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.Helpers;
using Paloma.Messages;

namespace Paloma.Views.Overlay.Chat;

public sealed partial class MarkdownView : ContentControl
{
    public static readonly DependencyProperty TextProperty = DependencyProperty.Register(
        nameof(Text),
        typeof(string),
        typeof(MarkdownView),
        new PropertyMetadata(string.Empty, (control, _) => ((MarkdownView)control).QueueRebuild()));

    private readonly StackPanel _container;
    private readonly MarkdownParser _parser;
    private readonly BatchedAction _rebuild;
    private readonly ThemeBrushManager _themeManager;

    private bool _hidden;

    public string Text
    {
        get => (string)GetValue(TextProperty);
        set => SetValue(TextProperty, value);
    }

    public MarkdownView()
    {
        _container = new StackPanel { Spacing = 6 };
        _parser = new MarkdownParser();
        _rebuild = new BatchedAction(Rebuild);
        _themeManager = new ThemeBrushManager(ActualTheme);

        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        HorizontalAlignment = HorizontalAlignment.Stretch;
        IsTabStop = false;
        Content = _container;

        ActualThemeChanged += (_, _) =>
        {
            _themeManager.Refresh(ActualTheme);
            _parser.Reset();
            QueueRebuild();
        };

        WeakReferenceMessenger.Default.Register<OverlayHiddenMessage>(this, (_, _) => _hidden = true);
        WeakReferenceMessenger.Default.Register<OverlayShownMessage>(this, (_, _) =>
        {
            _hidden = false;
            _rebuild.Trigger();
        });
    }

    // coalescing rendering event
    private void QueueRebuild()
    {
        // A hidden overlay skips per-delta rebuilds; one rebuild runs on show.
        if (_hidden)
        {
            return;
        }

        _rebuild.Trigger();
    }

    private void Rebuild()
    {
        var (keep, blocks) = _parser.RenderBlocks(Text);

        while (_container.Children.Count > keep)
        {
            _container.Children.RemoveAt(_container.Children.Count - 1);
        }

        foreach (var block in blocks)
        {
            _container.Children.Add(MarkdownRenderer.Render(block, _themeManager));
        }
    }
}