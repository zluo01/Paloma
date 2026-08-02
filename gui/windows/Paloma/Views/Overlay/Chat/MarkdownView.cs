using Windows.UI.Text;
using CommunityToolkit.Mvvm.Messaging;
using Markdig.Extensions.Tables;
using Markdig.Syntax;
using Markdig.Syntax.Inlines;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Paloma.Helpers;
using Paloma.Messages;
using Serilog;
using Block = Markdig.Syntax.Block;
using Inline = Microsoft.UI.Xaml.Documents.Inline;

namespace Paloma.Views.Overlay.Chat;

/// <summary>
/// Renders chat markdown as native blocks by mapping Markdig's syntax tree
/// to XAML. A delta re-renders only from the first changed block, so earlier
/// elements and any text selection in them survive streaming.
/// </summary>
public sealed partial class MarkdownView : ContentControl
{
    /// <summary>Columns at most this many characters hug their content;
    /// longer ones share leftover width and wrap.</summary>
    private const int SnugColumnChars = 24;

    public static readonly DependencyProperty TextProperty = DependencyProperty.Register(
        nameof(Text),
        typeof(string),
        typeof(MarkdownView),
        new PropertyMetadata(string.Empty, (control, _) => ((MarkdownView)control).QueueRebuild()));

    // Brushes resolve in code against the element's live theme. The theme
    // dictionary walk crosses WinRT interop, so results cache per key and theme.
    private static readonly Dictionary<(string Key, ElementTheme Theme), Brush> BrushCache = [];

    private readonly StackPanel _host = new() { Spacing = 6 };
    private readonly List<string> _renderedSources = [];
    private readonly BatchedAction _rebuild;
    private bool _themeDirty;
    private bool _hiddenDirty;

    public string Text
    {
        get => (string)GetValue(TextProperty);
        set => SetValue(TextProperty, value);
    }

    public MarkdownView()
    {
        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        HorizontalAlignment = HorizontalAlignment.Stretch;
        IsTabStop = false;
        Content = _host;
        _rebuild = new BatchedAction(Rebuild);
        // Brushes are resolved in code at build time, so a theme switch needs
        // a full rebuild to pick up the new ones.
        ActualThemeChanged += (_, _) =>
        {
            _themeDirty = true;
            QueueRebuild();
        };
        WeakReferenceMessenger.Default.Register<OverlayShownMessage>(this, (_, _) =>
        {
            if (!_hiddenDirty) return;
            _hiddenDirty = false;
            _rebuild.Trigger();
        });
    }

    /// <summary>Coalesces a burst of Text changes into one rebuild per
    /// dispatcher pass. Rebuilding per delta would starve the UI thread.</summary>
    private void QueueRebuild()
    {
        // A hidden overlay skips per-delta rebuilds; one rebuild runs on show.
        if (XamlRoot is { IsHostVisible: false })
        {
            _hiddenDirty = true;
            return;
        }

        _rebuild.Trigger();
    }

    private void Rebuild()
    {
        if (_themeDirty)
        {
            _themeDirty = false;
            _renderedSources.Clear();
            _host.Children.Clear();
        }

        IReadOnlyList<(string Source, Block Block)> blocks;
        try
        {
            blocks = MarkdownParser.Parse(Text);
        }
        catch (ArgumentException e)
        {
            // Invalid markdown string, show the raw data instead
            Log.Error(e, "markdown parse failed");
            _renderedSources.Clear();
            _host.Children.Clear();
            _host.Children.Add(new TextBlock
            {
                Text = Text,
                TextWrapping = TextWrapping.Wrap,
                IsTextSelectionEnabled = true,
            });
            return;
        }
        var keep = 0;
        while (keep < _renderedSources.Count
               && keep < blocks.Count
               && _renderedSources[keep] == blocks[keep].Source)
        {
            keep++;
        }

        if (keep == _renderedSources.Count - 1
            && keep == blocks.Count - 1
            && blocks[keep].Block is CodeBlock code
            && _host.Children[keep] is Border { Child: TextBlock body })
        {
            // The still-open fence grows in place, keeping its element and
            // any selection inside it.
            body.Text = code.Lines.ToString();
            _renderedSources[keep] = blocks[keep].Source;
            return;
        }

        while (_renderedSources.Count > keep)
        {
            _renderedSources.RemoveAt(_renderedSources.Count - 1);
            _host.Children.RemoveAt(_host.Children.Count - 1);
        }

        foreach (var (source, block) in blocks.Skip(keep))
        {
            _renderedSources.Add(source);
            _host.Children.Add(Render(block));
        }
    }

    private FrameworkElement Render(Block block)
    {
        return block switch
        {
            ParagraphBlock paragraph => InlineText(paragraph.Inline, 14),
            HeadingBlock heading => RenderHeading(heading),
            CodeBlock code => RenderCode(code.Lines.ToString()),
            QuoteBlock quote => RenderQuote(quote),
            ListBlock list => RenderList(list, depth: 0),
            Table table => RenderTable(table),
            ThematicBreakBlock => new Border
            {
                Height = 1,
                Margin = new Thickness(0, 4, 0, 4),
                Background = Resource("DividerStrokeColorDefaultBrush"),
            },
            // Raw HTML and anything else unstyled: its text, unformatted.
            LeafBlock leaf => new TextBlock
            {
                Text = leaf.Lines.ToString(),
                FontSize = 14,
                TextWrapping = TextWrapping.Wrap,
                IsTextSelectionEnabled = true,
            },
            _ => new TextBlock(),
        };
    }

    private Border RenderCode(string content)
    {
        return new Border
        {
            Background = Resource("SubtleFillColorSecondaryBrush"),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(10, 8, 10, 8),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            Child = new TextBlock
            {
                Text = content,
                FontFamily = MonoFont,
                FontSize = 12,
                TextWrapping = TextWrapping.Wrap,
                IsTextSelectionEnabled = true,
            },
        };
    }

    private Grid RenderQuote(QuoteBlock quote)
    {
        var grid = new Grid { ColumnSpacing = 8 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        grid.ColumnDefinitions.Add(
            new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var rule = new Rectangle
        {
            Width = 3,
            RadiusX = 1.5,
            RadiusY = 1.5,
            Fill = Resource("DividerStrokeColorDefaultBrush"),
            VerticalAlignment = VerticalAlignment.Stretch,
        };
        var body = new StackPanel { Spacing = 6 };
        foreach (var child in quote)
        {
            body.Children.Add(Render(child));
        }

        // Foreground inherits down to every text block the quote contains.
        var content = new ContentPresenter
        {
            Content = body,
            Foreground = Resource("TextFillColorSecondaryBrush"),
        };
        Grid.SetColumn(content, 1);
        grid.Children.Add(rule);
        grid.Children.Add(content);
        return grid;
    }

    private StackPanel RenderList(ListBlock list, int depth)
    {
        var panel = new StackPanel { Spacing = 3 };
        // Count up from the first number. Markdig keeps the source numbers,
        // and the common "1. / 1. / 1." style must render 1. 2. 3.
        var order = list.OfType<ListItemBlock>().FirstOrDefault()?.Order ?? 1;
        foreach (var item in list.OfType<ListItemBlock>())
        {
            var marker = list.IsOrdered ? $"{order++}." : "•";
            foreach (var child in item)
            {
                if (child is ListBlock nested)
                {
                    panel.Children.Add(RenderList(nested, depth + 1));
                }
                else
                {
                    panel.Children.Add(ListRow(depth, marker, Render(child)));
                    // The marker belongs to the item's first line only.
                    marker = string.Empty;
                }
            }
        }

        return panel;
    }

    private Grid ListRow(int depth, string marker, FrameworkElement content)
    {
        var row = new Grid
        {
            ColumnSpacing = 6,
            Margin = new Thickness(depth * 14, 0, 0, 0),
        };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(
            new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var bullet = new TextBlock
        {
            Text = marker,
            FontSize = 14,
            Foreground = Resource("TextFillColorSecondaryBrush"),
        };
        Grid.SetColumn(content, 1);
        row.Children.Add(bullet);
        row.Children.Add(content);
        return row;
    }

    private Border RenderTable(Table table)
    {
        var rows = table.OfType<TableRow>().ToList();
        var columns = rows.Count == 0 ? 0 : rows.Max(row => row.Count);
        var grid = new Grid { ColumnSpacing = 12, RowSpacing = 4 };
        // Auto columns measure unconstrained, so long cells never wrap and a
        // wide table clips. Star-sizing long columns by content length gives
        // them the finite width wrapping needs.
        var longest = new int[columns];
        foreach (var row in rows)
        {
            for (var i = 0; i < row.Count; i++)
            {
                longest[i] = Math.Max(longest[i], row[i].Span.Length);
            }
        }

        for (var i = 0; i < columns; i++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition
            {
                Width = longest[i] > SnugColumnChars
                    ? new GridLength(longest[i], GridUnitType.Star)
                    : GridLength.Auto,
            });
        }

        var rowIndex = 0;
        foreach (var row in rows)
        {
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            for (var column = 0; column < row.Count; column++)
            {
                var cell = (TableCell)row[column];
                var text = InlineText((cell.FirstOrDefault() as ParagraphBlock)?.Inline, 12);
                if (row.IsHeader)
                {
                    text.FontWeight = FontWeights.SemiBold;
                }

                Grid.SetRow(text, rowIndex);
                Grid.SetColumn(text, column);
                grid.Children.Add(text);
            }

            rowIndex++;
            if (row.IsHeader)
            {
                grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
                var divider = new Border
                {
                    Height = 1,
                    Background = Resource("DividerStrokeColorDefaultBrush"),
                };
                Grid.SetRow(divider, rowIndex);
                Grid.SetColumnSpan(divider, Math.Max(columns, 1));
                grid.Children.Add(divider);
                rowIndex++;
            }
        }

        return new Border
        {
            Background = Resource("SubtleFillColorSecondaryBrush"),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(10, 8, 10, 8),
            HorizontalAlignment = HorizontalAlignment.Left,
            Child = grid,
        };
    }

    private Brush Resource(string key)
    {
        var theme = ActualTheme;
        if (BrushCache.TryGetValue((key, theme), out var cached))
        {
            return cached;
        }

        var brush = ResolveBrush(key, theme);
        BrushCache[(key, theme)] = brush;
        return brush;
    }

    private static TextBlock InlineText(ContainerInline? content, double size)
    {
        var text = new TextBlock
        {
            FontSize = size,
            TextWrapping = TextWrapping.Wrap,
            IsTextSelectionEnabled = true,
        };
        Materialize(text.Inlines, content);
        return text;
    }

    private static void Materialize(InlineCollection target, ContainerInline? content)
    {
        for (var inline = content?.FirstChild; inline is not null; inline = inline.NextSibling)
        {
            switch (inline)
            {
                case LiteralInline literal:
                    target.Add(new Run { Text = literal.Content.ToString() });
                    break;
                case CodeInline code:
                    target.Add(new Run { Text = code.Content, FontFamily = MonoFont });
                    break;
                case LineBreakInline:
                    target.Add(new LineBreak());
                    break;
                case HtmlEntityInline entity:
                    target.Add(new Run { Text = entity.Transcoded.ToString() });
                    break;
                case AutolinkInline autolink:
                    AddLink(target, new Run { Text = autolink.Url }, autolink.Url);
                    break;
                case EmphasisInline emphasis:
                {
                    var span = new Span();
                    if (emphasis.DelimiterCount >= 2)
                    {
                        span.FontWeight = FontWeights.SemiBold;
                    }
                    else
                    {
                        span.FontStyle = FontStyle.Italic;
                    }

                    Materialize(span.Inlines, emphasis);
                    target.Add(span);
                    break;
                }
                case LinkInline link:
                {
                    var label = new Span();
                    Materialize(label.Inlines, link);
                    if (link.IsImage)
                    {
                        // No inline images in a transcript; the alt text stands in.
                        target.Add(label);
                    }
                    else
                    {
                        AddLink(target, label, link.Url);
                    }

                    break;
                }
                case HtmlInline html:
                    target.Add(new Run { Text = html.Tag });
                    break;
                case ContainerInline container:
                    Materialize(target, container);
                    break;
            }
        }
    }

    private static void AddLink(InlineCollection target, Inline label, string? url)
    {
        if (Uri.TryCreate(url, UriKind.Absolute, out var uri))
        {
            var hyperlink = new Hyperlink { NavigateUri = uri };
            hyperlink.Inlines.Add(label);
            target.Add(hyperlink);
        }
        else
        {
            target.Add(label);
        }
    }

    private static TextBlock RenderHeading(HeadingBlock heading)
    {
        var text = InlineText(heading.Inline, heading.Level switch
        {
            1 => 18,
            2 => 16,
            _ => 14,
        });
        text.FontWeight = heading.Level == 1 ? FontWeights.Bold : FontWeights.SemiBold;
        text.Margin = new Thickness(0, 4, 0, 0);
        return text;
    }

    private static Brush ResolveBrush(string key, ElementTheme theme)
    {
        string[] names = theme == ElementTheme.Light ? ["Light", "Default"] : ["Dark", "Default"];
        foreach (var name in names)
        {
            if (FindThemeDictionary(Application.Current.Resources, name) is { } dictionary
                && dictionary.TryGetValue(key, out var themed)
                && themed is Brush brush)
            {
                return brush;
            }
        }

        return Application.Current.Resources.TryGetValue(key, out var value)
               && value is Brush plain
            ? plain
            : new SolidColorBrush(Colors.Transparent);
    }

    private static ResourceDictionary? FindThemeDictionary(ResourceDictionary root, string name)
    {
        if (root.ThemeDictionaries.TryGetValue(name, out var value)
            && value is ResourceDictionary themed)
        {
            return themed;
        }

        foreach (var merged in root.MergedDictionaries)
        {
            if (FindThemeDictionary(merged, name) is { } found)
            {
                return found;
            }
        }

        return null;
    }

    private static FontFamily MonoFont =>
        Application.Current.Resources.TryGetValue("PalomaMonoFontFamily", out var value)
        && value is FontFamily family
            ? family
            : new FontFamily("Consolas");
}
