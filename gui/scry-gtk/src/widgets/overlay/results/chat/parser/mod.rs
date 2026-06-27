use std::borrow::Cow;

use pulldown_cmark::{CodeBlockKind, Event as MdEvent, Options, Parser, Tag, TagEnd};

mod node;

pub(super) use node::Block;

pub(super) struct ParsedMarkdown {
    pub(super) blocks: Vec<Block>,
}

impl ParsedMarkdown {
    pub(super) fn parse(src: &str) -> Self {
        Self {
            blocks: parse_blocks(src),
        }
    }

    pub(super) fn top_level_blocks(&self) -> usize {
        self.blocks.len()
    }

    pub(super) fn node_count(&self) -> usize {
        let mut count = 0;
        let mut stack: Vec<&Block> = self.blocks.iter().collect();
        while let Some(block) = stack.pop() {
            count += 1;
            stack.extend(block.children());
        }
        count
    }
}

// --- parsing ------------------------------------------------------------

#[derive(Default)]
struct Builder {
    out: Vec<Block>,
    stack: Vec<Block>,
}

impl Builder {
    /// Where the next child node goes — the open node's children, or the root.
    fn target(&mut self) -> &mut Vec<Block> {
        match self.stack.last_mut() {
            Some(block) => block
                .children_mut()
                .expect("open container accepts children"),
            None => &mut self.out,
        }
    }

    fn push(&mut self, block: Block) {
        self.stack.push(block);
    }

    /// Pop the open node and attach it to its parent. Code keeps no trailing
    /// whitespace, matching how it streams.
    fn pop(&mut self) {
        if let Some(mut block) = self.stack.pop() {
            if let Block::Code { code, .. } = &mut block {
                *code = code.trim_end().to_string();
            }
            self.target().push(block);
        }
    }

    /// Pop a list, promoting a bullet list with task items to a task list (its
    /// kind is only known once the items are in).
    fn pop_list(&mut self) {
        if let Some(list) = self.stack.pop() {
            self.target().push(finalize_list(list));
        }
    }

    fn leaf(&mut self, block: Block) {
        self.target().push(block);
    }

    /// Append text, merging into a trailing `Text` node so pulldown's split runs
    /// (e.g. around `<`/`&`) collapse to one node.
    fn push_text(&mut self, text: &str) {
        let target = self.target();
        if let Some(Block::Text(last)) = target.last_mut() {
            last.push_str(text);
        } else {
            target.push(Block::Text(text.to_string()));
        }
    }

    /// Close a list item, wrapping its bare inline run into a paragraph. Tight
    /// items emit text with no `Paragraph` event, so without this their text would
    /// sit as loose inline nodes instead of a block.
    fn close_item(&mut self) {
        let Some(item) = self.stack.pop() else {
            return;
        };
        let item = match item {
            Block::Item(children) => Block::Item(wrap_inline_runs(children)),
            Block::TaskItem { checked, children } => Block::TaskItem {
                checked,
                children: wrap_inline_runs(children),
            },
            other => other,
        };
        self.target().push(item);
    }

    /// Turn the open item into a task item, keeping any children it already has.
    fn set_task(&mut self, checked: bool) {
        if let Some(top @ Block::Item(_)) = self.stack.last_mut()
            && let Block::Item(children) = std::mem::replace(top, Block::Rule)
        {
            *top = Block::TaskItem { checked, children };
        }
    }

    fn handle(&mut self, event: MdEvent<'_>) {
        match event {
            MdEvent::Start(tag) => self.start(tag),
            MdEvent::End(tag) => self.end(tag),
            MdEvent::Text(text) => match self.stack.last_mut() {
                Some(Block::Code { code, .. }) => code.push_str(&text),
                _ => self.push_text(&text),
            },
            MdEvent::Code(text) => self.leaf(Block::InlineCode(text.to_string())),
            MdEvent::SoftBreak => self.leaf(Block::SoftBreak),
            MdEvent::HardBreak => self.leaf(Block::HardBreak),
            MdEvent::Rule => self.leaf(Block::Rule),
            MdEvent::TaskListMarker(checked) => self.set_task(checked),
            _ => {},
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.push(Block::Paragraph(Vec::new())),
            Tag::Heading { level, .. } => self.push(Block::Heading {
                level,
                children: Vec::new(),
            }),
            Tag::BlockQuote(_) => self.push(Block::Quote(Vec::new())),
            Tag::List(Some(start)) => self.push(Block::OrderedList {
                start,
                items: Vec::new(),
            }),
            Tag::List(None) => self.push(Block::BulletList(Vec::new())),
            Tag::Item => self.push(Block::Item(Vec::new())),
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    },
                    CodeBlockKind::Indented => String::new(),
                };
                self.push(Block::Code {
                    language,
                    code: String::new(),
                });
            },
            Tag::Strong => self.push(Block::Strong(Vec::new())),
            Tag::Emphasis => self.push(Block::Emphasis(Vec::new())),
            Tag::Strikethrough => self.push(Block::Strikethrough(Vec::new())),
            Tag::Link { dest_url, .. } => self.push(Block::Link {
                url: dest_url.to_string(),
                children: Vec::new(),
            }),
            Tag::Table(alignments) => self.push(Block::Table {
                alignments,
                children: Vec::new(),
            }),
            Tag::TableHead => self.push(Block::TableHead(Vec::new())),
            Tag::TableRow => self.push(Block::TableRow(Vec::new())),
            Tag::TableCell => self.push(Block::TableCell(Vec::new())),
            _ => {},
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::Strong
            | TagEnd::Emphasis
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell => self.pop(),
            TagEnd::Item => self.close_item(),
            TagEnd::List(_) => self.pop_list(),
            _ => {},
        }
    }

    /// Unwind any still-open nodes so a partially streamed construct still renders.
    fn finish(mut self) -> Vec<Block> {
        while !self.stack.is_empty() {
            match self.stack.last() {
                Some(Block::OrderedList { .. } | Block::BulletList(_) | Block::TaskList(_)) => {
                    self.pop_list()
                },
                _ => self.pop(),
            }
        }
        self.out
    }
}

/// Promote a bullet list that picked up task items to a task list.
fn finalize_list(list: Block) -> Block {
    match list {
        Block::BulletList(items) if items.iter().any(|i| matches!(i, Block::TaskItem { .. })) => {
            Block::TaskList(items)
        },
        other => other,
    }
}

/// Group consecutive inline nodes into paragraphs, leaving block nodes as-is.
fn wrap_inline_runs(children: Vec<Block>) -> Vec<Block> {
    let mut out = Vec::new();
    let mut run: Vec<Block> = Vec::new();
    for child in children {
        if is_inline(&child) {
            run.push(child);
        } else {
            if !run.is_empty() {
                out.push(Block::Paragraph(std::mem::take(&mut run)));
            }
            out.push(child);
        }
    }
    if !run.is_empty() {
        out.push(Block::Paragraph(run));
    }
    out
}

fn is_inline(block: &Block) -> bool {
    matches!(
        block,
        Block::Text(_)
            | Block::Strong(_)
            | Block::Emphasis(_)
            | Block::Strikethrough(_)
            | Block::Link { .. }
            | Block::InlineCode(_)
            | Block::SoftBreak
            | Block::HardBreak
    )
}

fn parse_blocks(src: &str) -> Vec<Block> {
    let src = unwrap_markdown_table_fences(src);
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut builder = Builder::default();
    for event in Parser::new_ext(&src, options) {
        builder.handle(event);
    }
    builder.finish()
}

// --- model fence unwrapping ----------------------------------------------

/// Models sometimes wrap whole tables in ```markdown fences; unwrap them
/// so the table renders instead of showing as a code block.
fn unwrap_markdown_table_fences(src: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut changed = false;
    let mut lines = src.split_inclusive('\n').peekable();

    while let Some(line) = lines.next() {
        if !is_markdown_fence_start(line) {
            out.push_str(line);
            continue;
        }

        let mut inner = String::new();
        let mut closing = None;
        for fenced_line in lines.by_ref() {
            if is_fence_end(fenced_line) {
                closing = Some(fenced_line);
                break;
            }
            inner.push_str(fenced_line);
        }

        if closing.is_some() && has_markdown_table(&inner) {
            out.push_str(&inner);
            changed = true;
        } else {
            out.push_str(line);
            out.push_str(&inner);
            if let Some(closing) = closing {
                out.push_str(closing);
            }
        }
    }

    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(src)
    }
}

fn is_markdown_fence_start(line: &str) -> bool {
    let trimmed = line.trim();
    let info = match trimmed.strip_prefix("```") {
        Some(info) => info.trim().to_ascii_lowercase(),
        None => return false,
    };
    matches!(info.as_str(), "md" | "markdown")
}

fn is_fence_end(line: &str) -> bool {
    line.trim().starts_with("```")
}

fn has_markdown_table(src: &str) -> bool {
    let mut previous_has_cells = false;
    for line in src.lines() {
        let has_cells = line.trim().contains('|');
        if previous_has_cells && is_table_delimiter(line) {
            return true;
        }
        previous_has_cells = has_cells;
    }
    false
}

fn is_table_delimiter(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|')
        && trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
        && trimmed.chars().filter(|c| *c == '-').count() >= 3
}

#[cfg(test)]
mod tests {
    use pulldown_cmark::{Alignment, HeadingLevel};

    use super::*;

    fn text(s: &str) -> Block {
        Block::Text(s.to_string())
    }

    fn para(children: Vec<Block>) -> Block {
        Block::Paragraph(children)
    }

    fn list_items(block: &Block) -> &[Block] {
        match block {
            Block::OrderedList { items, .. } => items,
            Block::BulletList(items) | Block::TaskList(items) => items,
            other => panic!("expected list, got {other:?}"),
        }
    }

    fn item_children(item: &Block) -> &[Block] {
        match item {
            Block::Item(children) | Block::TaskItem { children, .. } => children,
            other => panic!("expected item, got {other:?}"),
        }
    }

    fn nested_list(children: &[Block]) -> &Block {
        children
            .iter()
            .find(|c| {
                matches!(
                    c,
                    Block::BulletList(_) | Block::OrderedList { .. } | Block::TaskList(_)
                )
            })
            .expect("expected a nested list")
    }

    #[test]
    fn unwraps_markdown_fenced_table() {
        let src = "before\n```markdown\n| A | B |\n| --- | --- |\n| 1 | 2 |\n```\nafter";

        assert_eq!(
            unwrap_markdown_table_fences(src),
            "before\n| A | B |\n| --- | --- |\n| 1 | 2 |\nafter"
        );
    }

    /// Concatenated text of a table cell's inline content.
    fn cell_text(cell: &Block) -> String {
        fn walk(block: &Block, out: &mut String) {
            match block {
                Block::Text(t) | Block::InlineCode(t) => out.push_str(t),
                _ => block.children().iter().for_each(|c| walk(c, out)),
            }
        }
        let mut out = String::new();
        cell.children().iter().for_each(|c| walk(c, &mut out));
        out
    }

    #[test]
    fn table_cell_with_inline_markup_does_not_panic() {
        // Regression: pulldown emits Start/End(Strong|Emphasis|Strikethrough|Link)
        // inside a table cell. Cells are real containers now, so this parses
        // (no panic) and the text is preserved.
        for src in [
            "| **a** | b |\n| --- | --- |\n| 1 | 2 |",         // bold
            "| *a* | b |\n| --- | --- |\n| 1 | 2 |",           // italic
            "| ~~a~~ | b |\n| --- | --- |\n| 1 | 2 |",         // strikethrough
            "| [a](http://x) | b |\n| --- | --- |\n| 1 | 2 |", // link
        ] {
            let blocks = parse_blocks(src);
            let rows = blocks[0].children();
            assert!(matches!(rows[0], Block::TableHead(_)), "head for {src:?}");
            assert!(matches!(rows[1], Block::TableRow(_)), "body for {src:?}");
            let head = rows[0].children();
            assert_eq!(cell_text(&head[0]), "a", "header cell for {src:?}");
            assert_eq!(cell_text(&head[1]), "b", "header cell for {src:?}");
            let body = rows[1].children();
            assert_eq!(cell_text(&body[0]), "1", "body cell for {src:?}");
            assert_eq!(cell_text(&body[1]), "2", "body cell for {src:?}");
        }
    }

    #[test]
    fn table_cell_preserves_inline_structure() {
        let blocks = parse_blocks("| **a** | b |\n| --- | --- |\n| 1 | 2 |");
        // The bold header cell keeps a Strong node — not flattened to plain text.
        let Block::TableCell(inline) = &blocks[0].children()[0].children()[0] else {
            panic!("expected a table cell, got {:?}", blocks[0]);
        };
        assert_eq!(inline, &vec![Block::Strong(vec![text("a")])]);
    }

    #[test]
    fn table_captures_column_alignments() {
        let blocks = parse_blocks("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |");
        let Block::Table { alignments, .. } = &blocks[0] else {
            panic!("expected a table, got {:?}", blocks[0]);
        };
        assert_eq!(
            alignments,
            &vec![Alignment::Left, Alignment::Center, Alignment::Right]
        );
    }

    #[test]
    fn leaves_non_table_fence_alone() {
        let src = "```markdown\n# Title\nbody\n```";

        assert_eq!(unwrap_markdown_table_fences(src), src);
    }

    #[test]
    fn parses_paragraph_with_inline_markup() {
        let blocks = parse_blocks("hello **bold** world");

        assert_eq!(
            blocks,
            vec![para(vec![
                text("hello "),
                Block::Strong(vec![text("bold")]),
                text(" world"),
            ])]
        );
    }

    #[test]
    fn keeps_text_raw_unescaped() {
        // Escaping is the renderer's job; the tree keeps raw text.
        let blocks = parse_blocks("a < b & c");

        assert_eq!(blocks, vec![para(vec![text("a < b & c")])]);
    }

    #[test]
    fn parses_inline_code_span() {
        let blocks = parse_blocks("call `foo`");

        assert_eq!(
            blocks,
            vec![para(vec![
                text("call "),
                Block::InlineCode("foo".to_string())
            ])]
        );
    }

    #[test]
    fn parses_fenced_code_block_with_language() {
        let blocks = parse_blocks("```rust\nfn main() {}\n```");

        assert_eq!(
            blocks,
            vec![Block::Code {
                language: "rust".to_string(),
                code: "fn main() {}".to_string(),
            }]
        );
    }

    #[test]
    fn parses_unclosed_code_fence_while_streaming() {
        let blocks = parse_blocks("```rust\nfn main(");

        assert_eq!(
            blocks,
            vec![Block::Code {
                language: "rust".to_string(),
                code: "fn main(".to_string(),
            }]
        );
    }

    #[test]
    fn parses_table_with_header() {
        let blocks = parse_blocks("| A | B |\n| --- | --- |\n| 1 | 2 |");

        assert_eq!(
            blocks,
            vec![Block::Table {
                alignments: vec![Alignment::None, Alignment::None],
                children: vec![
                    Block::TableHead(vec![
                        Block::TableCell(vec![text("A")]),
                        Block::TableCell(vec![text("B")]),
                    ]),
                    Block::TableRow(vec![
                        Block::TableCell(vec![text("1")]),
                        Block::TableCell(vec![text("2")]),
                    ]),
                ],
            }]
        );
    }

    #[test]
    fn heading_keeps_level() {
        let blocks = parse_blocks("## Title");

        assert_eq!(
            blocks,
            vec![Block::Heading {
                level: HeadingLevel::H2,
                children: vec![text("Title")],
            }]
        );
    }

    #[test]
    fn parses_tight_bullet_list() {
        let blocks = parse_blocks("- one\n- two");

        assert!(matches!(&blocks[0], Block::BulletList(_)));
        let items = list_items(&blocks[0]);
        assert_eq!(item_children(&items[0]), &[para(vec![text("one")])]);
        assert_eq!(item_children(&items[1]), &[para(vec![text("two")])]);
    }

    #[test]
    fn ordered_list_keeps_start() {
        let blocks = parse_blocks("9. a\n10. b\n11. c");

        let Block::OrderedList { start, items } = &blocks[0] else {
            panic!("expected ordered list, got {blocks:?}");
        };
        assert_eq!(*start, 9);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn task_list_item_checked() {
        let blocks = parse_blocks("- [x] done");

        assert!(matches!(&blocks[0], Block::TaskList(_)));
        let items = list_items(&blocks[0]);
        assert!(matches!(items[0], Block::TaskItem { checked: true, .. }));
    }

    #[test]
    fn task_list_item_unchecked() {
        let blocks = parse_blocks("- [ ] todo");

        assert!(matches!(&blocks[0], Block::TaskList(_)));
        let items = list_items(&blocks[0]);
        assert!(matches!(items[0], Block::TaskItem { checked: false, .. }));
    }

    #[test]
    fn nested_tight_list_lives_in_parent_item() {
        let blocks = parse_blocks("- a\n  - b");

        let items = list_items(&blocks[0]);
        let children = item_children(&items[0]);
        assert_eq!(children[0], para(vec![text("a")]));
        let inner = list_items(nested_list(children));
        assert_eq!(item_children(&inner[0]), &[para(vec![text("b")])]);
    }

    #[test]
    fn loose_item_keeps_multiple_paragraphs() {
        let blocks = parse_blocks("- one\n\n  two");

        let items = list_items(&blocks[0]);
        assert_eq!(
            item_children(&items[0]),
            &[para(vec![text("one")]), para(vec![text("two")])]
        );
    }

    #[test]
    fn blockquote_with_list_unwinds_at_eof() {
        let blocks = parse_blocks("> - a\n> - b");

        assert_eq!(blocks.len(), 1);
        let Block::Quote(children) = &blocks[0] else {
            panic!("expected quote, got {blocks:?}");
        };
        assert_eq!(list_items(&children[0]).len(), 2);
    }

    #[test]
    fn code_block_inside_list_item_attaches_to_item() {
        let blocks = parse_blocks("- ```rust\n  fn f() {}\n  ```");

        assert_eq!(blocks.len(), 1);
        let items = list_items(&blocks[0]);
        assert!(matches!(
            item_children(&items[0]).last(),
            Some(Block::Code { .. })
        ));
    }

    #[test]
    fn unclosed_list_item_while_streaming_still_renders() {
        let blocks = parse_blocks("- one");

        let items = list_items(&blocks[0]);
        assert_eq!(item_children(&items[0]), &[para(vec![text("one")])]);
    }

    #[test]
    fn each_heading_level_is_preserved() {
        let cases = [
            ("# a", HeadingLevel::H1),
            ("## a", HeadingLevel::H2),
            ("### a", HeadingLevel::H3),
            ("#### a", HeadingLevel::H4),
            ("##### a", HeadingLevel::H5),
            ("###### a", HeadingLevel::H6),
        ];
        for (src, level) in cases {
            assert_eq!(
                parse_blocks(src),
                vec![Block::Heading {
                    level,
                    children: vec![text("a")],
                }],
                "for {src:?}"
            );
        }
    }

    #[test]
    fn table_inside_list_item_attaches_to_item() {
        let src = "- text\n\n  | A | B |\n  | --- | --- |\n  | 1 | 2 |";
        let blocks = parse_blocks(src);

        assert_eq!(
            blocks.len(),
            1,
            "table must not leak to the root: {blocks:?}"
        );
        let items = list_items(&blocks[0]);
        assert!(
            item_children(&items[0])
                .iter()
                .any(|b| matches!(b, Block::Table { .. })),
            "table should attach to the item: {:?}",
            item_children(&items[0])
        );
    }

    #[test]
    fn table_inside_quote_attaches_to_quote() {
        let src = "> | A | B |\n> | --- | --- |\n> | 1 | 2 |";
        let blocks = parse_blocks(src);

        let Block::Quote(children) = &blocks[0] else {
            panic!("expected quote, got {blocks:?}");
        };
        assert!(
            children.iter().any(|b| matches!(b, Block::Table { .. })),
            "table should attach to the quote: {children:?}"
        );
    }

    #[test]
    fn deeply_nested_list_unwinds_at_eof() {
        let blocks = parse_blocks("- a\n  - b\n    - c");

        let l1 = list_items(&blocks[0]);
        let l2 = list_items(nested_list(item_children(&l1[0])));
        let l3 = list_items(nested_list(item_children(&l2[0])));
        assert_eq!(item_children(&l3[0]), &[para(vec![text("c")])]);
    }
}
