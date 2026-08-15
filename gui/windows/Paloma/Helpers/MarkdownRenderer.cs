using Windows.UI.Text;
using Markdig.Extensions.Tables;
using Markdig.Syntax;
using Markdig.Syntax.Inlines;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Block = Markdig.Syntax.Block;
using Inline = Microsoft.UI.Xaml.Documents.Inline;

namespace Paloma.Helpers;

internal static class MarkdownRenderer
{
    /// A table column with only short text keeps the exact width of that text.
    /// Columns with longer text split the leftover width and wrap.
    private const int SnugColumnChars = 24;

    private static readonly FontFamily MonoFont =
        Application.Current.Resources.TryGetValue("PalomaMonoFontFamily", out var value)
        && value is FontFamily family
            ? family
            : new FontFamily("Consolas");

    public static FrameworkElement Render(Block block, ThemeBrushManager manager)
    {
        return block switch
        {
            ParagraphBlock paragraph => InlineText(paragraph.Inline, 14),
            HeadingBlock heading => RenderHeading(heading),
            CodeBlock code => RenderCode(code.Lines.ToString(), manager),
            QuoteBlock quote => RenderQuote(quote, manager),
            ListBlock list => RenderList(list, depth: 0, manager),
            Table table => RenderTable(table, manager),
            ThematicBreakBlock => new Border
            {
                Height = 1,
                Margin = new Thickness(0, 4, 0, 4),
                Background = manager.ThemeBrush("DividerStrokeColorDefaultBrush"),
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

    private static Border RenderCode(string content, ThemeBrushManager manager)
    {
        return new Border
        {
            Background = manager.ThemeBrush("SubtleFillColorSecondaryBrush"),
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

    private static Grid RenderQuote(QuoteBlock quote, ThemeBrushManager manager)
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
            Fill = manager.ThemeBrush("DividerStrokeColorDefaultBrush"),
            VerticalAlignment = VerticalAlignment.Stretch,
        };
        var body = new StackPanel { Spacing = 6 };
        foreach (var child in quote)
        {
            body.Children.Add(Render(child, manager));
        }

        // Foreground inherits down to every text block the quote contains.
        var content = new ContentPresenter
        {
            Content = body,
            Foreground = manager.ThemeBrush("TextFillColorSecondaryBrush"),
        };
        Grid.SetColumn(content, 1);
        grid.Children.Add(rule);
        grid.Children.Add(content);
        return grid;
    }

    private static StackPanel RenderList(ListBlock list, int depth, ThemeBrushManager manager)
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
                    panel.Children.Add(RenderList(nested, depth + 1, manager));
                }
                else
                {
                    panel.Children.Add(ListRow(depth, marker, Render(child, manager), manager));
                    // The marker belongs to the item's first line only.
                    marker = string.Empty;
                }
            }
        }

        return panel;
    }

    private static Grid ListRow(
        int depth, string marker, FrameworkElement content, ThemeBrushManager manager)
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
            Foreground = manager.ThemeBrush("TextFillColorSecondaryBrush"),
        };
        Grid.SetColumn(content, 1);
        row.Children.Add(bullet);
        row.Children.Add(content);
        return row;
    }

    private static Border RenderTable(Table table, ThemeBrushManager manager)
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
            if (!row.IsHeader) continue;
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var divider = new Border
            {
                Height = 1,
                Background = manager.ThemeBrush("DividerStrokeColorDefaultBrush"),
            };
            Grid.SetRow(divider, rowIndex);
            Grid.SetColumnSpan(divider, Math.Max(columns, 1));
            grid.Children.Add(divider);
            rowIndex++;
        }

        return new Border
        {
            Background = manager.ThemeBrush("SubtleFillColorSecondaryBrush"),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(10, 8, 10, 8),
            HorizontalAlignment = HorizontalAlignment.Left,
            Child = grid,
        };
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
}
