using Markdig.Extensions.Tables;
using Markdig.Syntax;
using Markdig.Syntax.Inlines;
using Paloma.Helpers;
using Xunit;

namespace Paloma.Tests;

public sealed class MarkdownParserTests
{
    private static readonly int[] RepeatedOnes = [1, 1, 1];

    [Fact]
    public void RenderBlocks_StripsCitationsCompleteMidStreamAndStray()
    {
        Assert.Equal(
            "done.  next",
            LiteralText(Single("done. citeturn0search1 next")));
        Assert.Equal("done.", LiteralText(Single("done.citeturn0sea")));
        Assert.Equal(
            "Stray  token and  closer.",
            LiteralText(Single("Stray  token and  closer.")));
    }

    [Fact]
    public void RenderBlocks_PipeTables_AreEnabledInThePipeline()
    {
        var table = Assert.IsType<Table>(Single("| Name | Value |\n| --- | --- |\n| a | 1 |"));

        var rows = table.Cast<TableRow>().ToList();
        Assert.Equal(2, rows.Count);
        Assert.True(rows[0].IsHeader);
    }

    [Fact]
    public void RenderBlocks_UnterminatedFence_StillStreamingParsesAsCode()
    {
        var code = Assert.IsType<FencedCodeBlock>(Single("```\nvar x = 1;\nvar y"));

        Assert.Equal("var x = 1;\nvar y", code.Lines.ToString());
    }

    [Fact]
    public void RenderBlocks_OrderedListOfRepeatedOnes_ParsesAsOneListWithSourceNumbers()
    {
        var list = Assert.IsType<ListBlock>(Single("1. alpha\n1. beta\n1. gamma"));

        Assert.Equal(RepeatedOnes, list.OfType<ListItemBlock>().Select(item => item.Order));
    }

    [Fact]
    public void RenderBlocks_OrderedList_KeepsTheFirstNumberAsTheStart()
    {
        var list = Assert.IsType<ListBlock>(Single("3. alpha\n1. beta\n8. gamma"));

        Assert.Equal(3, list.OfType<ListItemBlock>().First().Order);
    }

    [Fact]
    public void RenderBlocks_FirstCall_ReturnsEveryBlock()
    {
        var parser = new MarkdownParser();

        var (keep, blocks) = parser.RenderBlocks("first.\n\nsecond.\n\nthird.");

        Assert.Equal(0, keep);
        Assert.Equal(3, blocks.Count);
    }

    [Fact]
    public void RenderBlocks_GrowingTheLastBlock_ReturnsOnlyThatBlock()
    {
        var parser = new MarkdownParser();
        parser.RenderBlocks("first.\n\nsecond");

        var (keep, blocks) = parser.RenderBlocks("first.\n\nsecond grows");

        Assert.Equal(1, keep);
        Assert.Equal("second grows", LiteralText(Assert.Single(blocks)));
    }

    [Fact]
    public void RenderBlocks_DeltaAddingABlock_ReturnsTheSettledAndTheNewBlock()
    {
        var parser = new MarkdownParser();
        parser.RenderBlocks("first.");

        // The same delta can extend the old last block and open a new one,
        // so the render must restart from the old last block.
        var (keep, blocks) = parser.RenderBlocks("first. done\n\nsecond");

        Assert.Equal(0, keep);
        Assert.Equal(2, blocks.Count);
        Assert.Equal("first. done", LiteralText(blocks[0]));
        Assert.Equal("second", LiteralText(blocks[1]));
    }

    [Fact]
    public void RenderBlocks_DeltaAddingSeveralBlocks_ReturnsAllFromTheSettledBlock()
    {
        var parser = new MarkdownParser();
        parser.RenderBlocks("first.\n\nsecond");

        var (keep, blocks) = parser.RenderBlocks("first.\n\nsecond.\n\nthird.\n\nfourth.");

        Assert.Equal(1, keep);
        Assert.Equal(3, blocks.Count);
    }

    [Fact]
    public void RenderBlocks_AfterReset_ReturnsEveryBlock()
    {
        var parser = new MarkdownParser();
        parser.RenderBlocks("first.\n\nsecond.");
        parser.Reset();

        var (keep, blocks) = parser.RenderBlocks("first.\n\nsecond.");

        Assert.Equal(0, keep);
        Assert.Equal(2, blocks.Count);
    }

    [Fact]
    public void RenderBlocks_ShrinkingParse_KeepsNothingAndReturnsTheFullList()
    {
        var parser = new MarkdownParser();
        parser.RenderBlocks("first.\n\nsecond.\n\nthird.");

        var (keep, blocks) = parser.RenderBlocks("only.");

        Assert.Equal(0, keep);
        Assert.Equal("only.", LiteralText(Assert.Single(blocks)));
    }

    [Fact]
    public void RenderBlocks_WhenTheParserThrows_FallsBackToTheRawText()
    {
        var invalid = new string('>', 10_000) + " deep";
        var parser = new MarkdownParser();

        var (keep, blocks) = parser.RenderBlocks(invalid);

        Assert.Equal(0, keep);
        Assert.Equal(invalid, LiteralText(Assert.Single(blocks)));
    }

    [Fact]
    public void RenderBlocks_ParseFailureAfterRenderedBlocks_KeepsNothing()
    {
        var parser = new MarkdownParser();
        parser.RenderBlocks("a.\n\nb.\n\nc.\n\nd.\n\ne.");

        // The raw-text fallback is one block, so the shrinking count must
        // clear the five already rendered ones instead of appending to them.
        var invalid = new string('>', 10_000) + " deep";
        var (keep, blocks) = parser.RenderBlocks(invalid);

        Assert.Equal(0, keep);
        Assert.Equal(invalid, LiteralText(Assert.Single(blocks)));
    }

    [Fact]
    public void RenderBlocks_EmptyText_ReturnsNothingAndKeepsNothing()
    {
        var parser = new MarkdownParser();

        var (keep, blocks) = parser.RenderBlocks(string.Empty);

        Assert.Equal(0, keep);
        Assert.Empty(blocks);
    }

    [Fact]
    public void RenderBlocks_ShrinkToEmpty_ClearsTheRenderedBlocks()
    {
        var parser = new MarkdownParser();
        parser.RenderBlocks("first.\n\nsecond.");

        var (keep, blocks) = parser.RenderBlocks(string.Empty);

        Assert.Equal(0, keep);
        Assert.Empty(blocks);
    }

    private static Block Single(string text)
    {
        return Assert.Single(new MarkdownParser().RenderBlocks(text).Blocks);
    }

    private static string LiteralText(Block block)
    {
        var paragraph = Assert.IsType<ParagraphBlock>(block);
        return string.Concat(
            paragraph.Inline!.Descendants<LiteralInline>().Select(l => l.Content.ToString()));
    }
}
