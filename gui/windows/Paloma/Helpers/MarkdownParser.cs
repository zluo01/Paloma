using System.Text.RegularExpressions;
using Markdig;
using Markdig.Syntax;
using Markdig.Syntax.Inlines;
using Serilog;

namespace Paloma.Helpers;

public partial class MarkdownParser
{
    private static readonly MarkdownPipeline Pipeline =
        new MarkdownPipelineBuilder().UsePipeTables().Build();

    // Strip Web-search citations
    [GeneratedRegex("\uE200[^\uE201]*(?:\uE201|$)|[\uE200-\uE202]")]
    private static partial Regex CitationPattern { get; }

    private int _renderCount;

    public void Reset()
    {
        _renderCount = 0;
    }

    public (int Keep, IReadOnlyList<Block> Blocks) RenderBlocks(string text)
    {
        var blocks = Parse(text);

        // blocks should always be >= current count,
        // if smaller, can mean error or shift of block type (paragraph to link), keep nothing, rerender everything
        // else, we render start from the last blocks
        var keep = blocks.Count < _renderCount ? 0 : Math.Max(_renderCount - 1, 0);
        _renderCount = blocks.Count;
        return (keep, [.. blocks.Skip(keep)]);
    }

    private static IReadOnlyList<Block> Parse(string text)
    {
        try
        {
            var stripped = CitationPattern.Replace(text, string.Empty);
            return
            [
                .. Markdown.Parse(stripped, Pipeline)
                    .Where(block => block is not LinkReferenceDefinitionGroup)
            ];
        }
        catch (ArgumentException e)
        {
            // Invalid Markdown string, show the raw data instead
            Log.Error(e, "markdown parse failed");
            var raw = new ParagraphBlock { Inline = new ContainerInline() };
            raw.Inline.AppendChild(new LiteralInline(text));
            return [raw];
        }
    }
}