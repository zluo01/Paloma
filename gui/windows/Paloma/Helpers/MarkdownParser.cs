using System.Text.RegularExpressions;
using Markdig;
using Markdig.Syntax;

namespace Paloma.Helpers;

public static partial class MarkdownParser
{
    private static readonly MarkdownPipeline Pipeline =
        new MarkdownPipelineBuilder().UsePipeTables().Build();

    // Strip Web-search citations
    [GeneratedRegex("\uE200[^\uE201]*(?:\uE201|$)|[\uE200-\uE202]")]
    private static partial Regex CitationPattern { get; }

    public static IReadOnlyList<(string Source, Block Block)> Parse(string text)
    {
        var stripped = CitationPattern.Replace(text, string.Empty);
        return
        [
            .. Markdown.Parse(stripped, Pipeline)
                .Where(block => block is not LinkReferenceDefinitionGroup)
                .Select(block => (Slice(stripped, block.Span), block))
        ];
    }

    // For ongoing block, render it as normal string
    private static string Slice(string text, SourceSpan span)
    {
        return span.Start >= 0 && span.Start < text.Length
            ? text.Substring(span.Start, Math.Min(span.Length, text.Length - span.Start))
            : string.Empty;
    }
}