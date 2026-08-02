using Markdig.Extensions.Tables;
using Markdig.Syntax;
using Paloma.Helpers;
using Xunit;

namespace Paloma.Tests;

public sealed class MarkdownParserTests
{

    private static readonly int[] RepeatedOnes = [1, 1, 1];

    [Fact]
    public void Parse_StripsCitationsCompleteMidStreamAndStray()
    {
        Assert.Equal("done.  next", Source("done. \uE200cite\uE202turn0search1\uE201 next"));
        Assert.Equal("done.", Source("done.\uE200cite\uE202turn0sea"));
        Assert.Equal("Stray  token and  closer.", Source("Stray \uE202 token and \uE201 closer."));
    }

    [Fact]
    public void PipeTables_AreEnabledInThePipeline()
    {
        var blocks = MarkdownParser.Parse("| Name | Value |\n| --- | --- |\n| a | 1 |");

        var table = Assert.IsType<Table>(Assert.Single(blocks).Block);
        var rows = table.Cast<TableRow>().ToList();
        Assert.Equal(2, rows.Count);
        Assert.True(rows[0].IsHeader);
    }

    [Fact]
    public void UnterminatedFence_StillStreamingParsesAsCode()
    {
        var blocks = MarkdownParser.Parse("```\nvar x = 1;\nvar y");

        var code = Assert.IsType<FencedCodeBlock>(Assert.Single(blocks).Block);
        Assert.Equal("var x = 1;\nvar y", code.Lines.ToString());
    }

    // The behavior MarkdownView's list numbering rides on: repeated "1."
    // items parse as ONE list carrying the source numbers verbatim, so the
    // renderer must count up from the first item itself.
    [Fact]
    public void OrderedList_RepeatedOnes_ParseAsOneListWithSourceNumbers()
    {
        var blocks = MarkdownParser.Parse("1. alpha\n1. beta\n1. gamma");

        var list = Assert.IsType<ListBlock>(Assert.Single(blocks).Block);
        Assert.Equal(RepeatedOnes, list.OfType<ListItemBlock>().Select(item => item.Order));
    }

    [Fact]
    public void OrderedList_KeepsTheFirstNumberAsTheStart()
    {
        var blocks = MarkdownParser.Parse("3. alpha\n1. beta\n8. gamma");

        var list = Assert.IsType<ListBlock>(Assert.Single(blocks).Block);
        Assert.Equal(3, list.OfType<ListItemBlock>().First().Order);
    }

    // The property MarkdownView's block reuse rides on: growing the text
    // only ever changes the block still being extended, never the slices of
    // the blocks before it.
    [Fact]
    public void Append_KeepsEarlierBlockSlicesStable()
    {
        const string document =
            "# Title\nProse with **bold**, `code` and a [link](https://x.dev).\n\n"
            + "- one\n- two\n  - nested\n\n> quoted\n\n"
            + "| a | b |\n| --- | --- |\n| 1 | 2 |\n\n"
            + "```csharp\nvar a = 1;\n```\nCited\uE200cite\uE202token\uE201 tail.";
        var previous = MarkdownParser.Parse(string.Empty);
        for (var end = 1; end <= document.Length; end++)
        {
            var current = MarkdownParser.Parse(document[..end]);
            for (var i = 0; i < Math.Min(previous.Count, current.Count) - 1; i++)
            {
                Assert.Equal(previous[i].Source, current[i].Source);
            }

            previous = current;
        }
    }

    private static string Source(string text)
    {
        return Assert.Single(MarkdownParser.Parse(text)).Source;
    }
}