//! Widget-per-block markdown rendering for assistant output.
//!
//! The accumulated source is parsed into a tree of semantic [`Block`]s by a small
//! frame-stack parser, then rendered to one widget per top-level block.
//! [`MarkdownView`] keeps one widget per top-level block and, on each streaming
//! update, rebuilds only from the first changed block. During append-streaming
//! that is usually just the last block, so completed blocks and selections stay
//! untouched.
//!
//! Rendering is split at the [`ParsedMarkdown`] boundary: a pure parse step
//! ([`ParsedMarkdown::parse`] — no GTK objects, owned `Send` output) and a GTK
//! apply step ([`MarkdownView::apply_parsed`]). The streaming path parses while
//! holding only a brief source borrow, then applies after it is dropped.
//! (`parse_blocks` and `MarkdownView::apply` are the internals behind that
//! boundary.)

use std::borrow::Cow;

use gtk4::{
    Align, Box as GtkBox, CheckButton, Grid, Label, Orientation, Separator, Widget, pango,
    prelude::*,
};
use pulldown_cmark::{
    Alignment, CodeBlockKind, Event as MdEvent, HeadingLevel, Options, Parser, Tag, TagEnd,
};

/// Inline code span: monospace on a faint neutral chip.
const INLINE_CODE_OPEN: &str = "<span background=\"#888888\" bgalpha=\"20%\"><tt>";
const INLINE_CODE_CLOSE: &str = "</tt></span>";

pub(super) struct MarkdownView {
    pub(super) widget: GtkBox,
    blocks: Vec<Rendered>,
}

struct Rendered {
    block: Block,
    widget: Widget,
}

#[derive(Clone, PartialEq, Debug)]
enum Block {
    Paragraph {
        markup: String,
    },
    Heading {
        level: HeadingLevel,
        markup: String,
    },
    Quote(Vec<Block>),
    List(ListBlock),
    Code {
        language: String,
        code: String,
    },
    Table {
        alignments: Vec<ColumnAlign>,
        header_rows: usize,
        rows: Vec<Vec<String>>,
    },
    Rule,
}

#[derive(Clone, PartialEq, Debug)]
struct ListBlock {
    ordered: bool,
    items: Vec<ListItem>,
}

#[derive(Clone, PartialEq, Debug)]
struct ListItem {
    /// Bullet or ordinal (e.g. "•" or "3."); ignored for task items.
    marker: String,
    /// `Some(checked)` for `- [ ]` / `- [x]` items.
    task: Option<bool>,
    children: Vec<Block>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum ColumnAlign {
    Left,
    Center,
    Right,
}

impl From<Alignment> for ColumnAlign {
    fn from(alignment: Alignment) -> Self {
        match alignment {
            Alignment::Center => ColumnAlign::Center,
            Alignment::Right => ColumnAlign::Right,
            Alignment::None | Alignment::Left => ColumnAlign::Left,
        }
    }
}

impl MarkdownView {
    pub(super) fn new() -> Self {
        // Inter-block spacing is per-widget `margin_top` (see `gap_above`), so the
        // box itself adds none.
        let widget = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(0)
            .build();
        Self {
            widget,
            blocks: Vec::new(),
        }
    }

    /// Apply already-parsed markdown. The streaming path parses under a brief
    /// source borrow and drops it before calling this, and callers drop the `turns`
    /// borrow first — so no source or `turns` borrow is held across the GTK build.
    pub(super) fn apply_parsed(&mut self, parsed: ParsedMarkdown) {
        self.apply(parsed.blocks);
    }

    /// Apply a block list, rebuilding widgets only from the first block that
    /// changed. Keeping the unchanged prefix preserves completed widgets (and
    /// their selections) during streaming.
    fn apply(&mut self, next: Vec<Block>) {
        let keep = self
            .blocks
            .iter()
            .zip(&next)
            .take_while(|(rendered, block)| rendered.block == **block)
            .count();

        for rendered in self.blocks.drain(keep..) {
            self.widget.remove(&rendered.widget);
        }
        for block in next.into_iter().skip(keep) {
            let widget = build_block(&block);
            widget.set_margin_top(gap_above(self.blocks.last().map(|r| &r.block), &block));
            self.widget.append(&widget);
            self.blocks.push(Rendered { block, widget });
        }
    }
}

/// Opaque parsed markdown: a parse/apply boundary that keeps `Block` private
/// while letting the streaming path (`chat/mod.rs`) separate the source-borrowing
/// parse from the GTK apply, and read render-size stats for the trace.
pub(super) struct ParsedMarkdown {
    blocks: Vec<Block>,
}

impl ParsedMarkdown {
    pub(super) fn parse(src: &str) -> Self {
        Self {
            blocks: parse_blocks(src),
        }
    }

    /// Number of top-level blocks. Understates render size for a single large
    /// list/quote; pair with [`node_count`](Self::node_count). For the streaming
    /// trace (`chat/mod.rs`).
    pub(super) fn top_level_blocks(&self) -> usize {
        self.blocks.len()
    }

    /// Recursive count of widget-producing nodes — each block, each list item and
    /// its children, and table cells — so the streaming trace reflects real apply
    /// cost. Only for the trace; never on the normal render path.
    pub(super) fn node_count(&self) -> usize {
        count_nodes(&self.blocks)
    }
}

fn count_nodes(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::Quote(children) => 1 + count_nodes(children),
            Block::List(list) => {
                1 + list
                    .items
                    .iter()
                    .map(|item| 1 + count_nodes(&item.children))
                    .sum::<usize>()
            },
            Block::Table { rows, .. } => 1 + rows.iter().map(Vec::len).sum::<usize>(),
            _ => 1,
        })
        .sum()
}

// --- parsing ------------------------------------------------------------

/// Inline accumulation for the current text block. Inline events append here
/// unconditionally; this is why tight list items (which emit no `Paragraph`
/// events) still capture their text.
#[derive(Default)]
struct Inline {
    markup: String,
    heading: Option<HeadingLevel>,
}

/// A container on the block stack. `Document` is always at the bottom.
enum Frame {
    Document(Vec<Block>),
    Quote(Vec<Block>),
    List {
        ordered: bool,
        next_no: u64,
        items: Vec<ListItem>,
    },
    Item {
        marker: String,
        task: Option<bool>,
        children: Vec<Block>,
    },
}

struct BlockParser {
    stack: Vec<Frame>,
    inline: Inline,
    code: Option<(String, String)>,
    table: Option<TableState>,
}

impl BlockParser {
    fn new() -> Self {
        Self {
            stack: vec![Frame::Document(Vec::new())],
            inline: Inline::default(),
            code: None,
            table: None,
        }
    }

    /// The nearest block container (`Item`/`Quote`/`Document`), skipping `List`
    /// frames (a list holds items, not blocks).
    fn nearest_container(&mut self) -> &mut Vec<Block> {
        let idx = self
            .stack
            .iter()
            .rposition(|f| !matches!(f, Frame::List { .. }))
            .expect("Document frame is always at the bottom");
        match &mut self.stack[idx] {
            Frame::Document(v) | Frame::Quote(v) | Frame::Item { children: v, .. } => v,
            Frame::List { .. } => unreachable!("rposition skipped List frames"),
        }
    }

    fn append_block(&mut self, block: Block) {
        self.nearest_container().push(block);
    }

    /// Emit the accumulated inline run as a `Heading` or `Paragraph`.
    fn flush_inline(&mut self) {
        let markup = std::mem::take(&mut self.inline.markup);
        let heading = self.inline.heading.take();
        let trimmed = markup.trim_matches('\n');
        if trimmed.trim().is_empty() {
            return;
        }
        let block = match heading {
            Some(level) => Block::Heading {
                level,
                markup: trimmed.to_string(),
            },
            None => Block::Paragraph {
                markup: trimmed.to_string(),
            },
        };
        self.append_block(block);
    }

    /// Marker for a new item, from the enclosing `List` frame.
    fn next_marker(&mut self) -> String {
        match self.stack.last_mut() {
            Some(Frame::List {
                ordered: true,
                next_no,
                ..
            }) => {
                let marker = format!("{next_no}.");
                *next_no += 1;
                marker
            },
            _ => "•".to_string(),
        }
    }

    fn set_task(&mut self, checked: bool) {
        for frame in self.stack.iter_mut().rev() {
            if let Frame::Item { task, .. } = frame {
                *task = Some(checked);
                return;
            }
        }
    }

    fn close_item(&mut self) {
        self.flush_inline();
        let Some(Frame::Item {
            marker,
            task,
            children,
        }) = self.stack.pop()
        else {
            return;
        };
        if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
            items.push(ListItem {
                marker,
                task,
                children,
            });
        }
    }

    fn close_list(&mut self) {
        if let Some(Frame::List { ordered, items, .. }) = self.stack.pop() {
            self.append_block(Block::List(ListBlock { ordered, items }));
        }
    }

    fn close_quote(&mut self) {
        self.flush_inline();
        if let Some(Frame::Quote(children)) = self.stack.pop() {
            self.append_block(Block::Quote(children));
        }
    }

    fn handle(&mut self, event: MdEvent<'_>) {
        match event {
            MdEvent::Start(Tag::Table(alignments)) => {
                self.flush_inline();
                self.table = Some(TableState::new(alignments));
            },
            MdEvent::End(TagEnd::Table) => {
                if let Some(table) = self.table.take() {
                    self.append_block(table.into_block());
                }
            },
            event if self.table.is_some() => {
                if let Some(table) = self.table.as_mut() {
                    table.push_event(event);
                }
            },

            MdEvent::Start(Tag::CodeBlock(kind)) => {
                self.flush_inline();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    },
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
            },
            MdEvent::Text(text) if self.code.is_some() => {
                if let Some((_, body)) = self.code.as_mut() {
                    body.push_str(&text);
                }
            },
            MdEvent::End(TagEnd::CodeBlock) => {
                if let Some((language, body)) = self.code.take() {
                    self.append_block(Block::Code {
                        language,
                        code: body.trim_end().to_string(),
                    });
                }
            },

            MdEvent::Start(Tag::Paragraph) | MdEvent::End(TagEnd::Paragraph) => self.flush_inline(),

            MdEvent::Start(Tag::Heading { level, .. }) => {
                self.flush_inline();
                self.inline.heading = Some(level);
            },
            MdEvent::End(TagEnd::Heading(_)) => self.flush_inline(),

            MdEvent::Start(Tag::BlockQuote(_)) => {
                self.flush_inline();
                self.stack.push(Frame::Quote(Vec::new()));
            },
            MdEvent::End(TagEnd::BlockQuote(_)) => self.close_quote(),

            MdEvent::Start(Tag::List(start)) => {
                self.flush_inline();
                self.stack.push(Frame::List {
                    ordered: start.is_some(),
                    next_no: start.unwrap_or(0),
                    items: Vec::new(),
                });
            },
            MdEvent::End(TagEnd::List(_)) => self.close_list(),

            MdEvent::Start(Tag::Item) => {
                self.flush_inline();
                let marker = self.next_marker();
                self.stack.push(Frame::Item {
                    marker,
                    task: None,
                    children: Vec::new(),
                });
            },
            MdEvent::End(TagEnd::Item) => self.close_item(),
            MdEvent::TaskListMarker(checked) => self.set_task(checked),

            MdEvent::Start(Tag::Strong) => self.inline.markup.push_str("<b>"),
            MdEvent::End(TagEnd::Strong) => self.inline.markup.push_str("</b>"),
            MdEvent::Start(Tag::Emphasis) => self.inline.markup.push_str("<i>"),
            MdEvent::End(TagEnd::Emphasis) => self.inline.markup.push_str("</i>"),
            MdEvent::Start(Tag::Strikethrough) => self.inline.markup.push_str("<s>"),
            MdEvent::End(TagEnd::Strikethrough) => self.inline.markup.push_str("</s>"),
            MdEvent::Start(Tag::Link { dest_url, .. }) => {
                self.inline
                    .markup
                    .push_str(&format!("<a href=\"{}\">", escape(&dest_url)));
            },
            MdEvent::End(TagEnd::Link) => self.inline.markup.push_str("</a>"),
            MdEvent::Code(text) => {
                self.inline.markup.push_str(INLINE_CODE_OPEN);
                self.inline.markup.push_str(&escape(&text));
                self.inline.markup.push_str(INLINE_CODE_CLOSE);
            },
            MdEvent::Text(text) => self.inline.markup.push_str(&escape(&text)),
            MdEvent::SoftBreak | MdEvent::HardBreak => self.inline.markup.push('\n'),
            MdEvent::Rule => {
                self.flush_inline();
                self.append_block(Block::Rule);
            },

            _ => {},
        }
    }

    /// Close any open code/table and unwind the container stack top-down, so a
    /// partially streamed construct still renders.
    fn finish(mut self) -> Vec<Block> {
        if let Some((language, body)) = self.code.take() {
            self.append_block(Block::Code {
                language,
                code: body.trim_end().to_string(),
            });
        }
        if let Some(table) = self.table.take() {
            self.append_block(table.into_block());
        }
        self.flush_inline();

        while self.stack.len() > 1 {
            match self.stack.last().expect("len > 1") {
                Frame::Item { .. } => self.close_item(),
                Frame::List { .. } => self.close_list(),
                Frame::Quote(_) => self.close_quote(),
                Frame::Document(_) => break,
            }
        }
        match self.stack.pop() {
            Some(Frame::Document(blocks)) => blocks,
            _ => Vec::new(),
        }
    }
}

fn parse_blocks(src: &str) -> Vec<Block> {
    let src = unwrap_markdown_table_fences(src);
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut parser = BlockParser::new();
    for event in Parser::new_ext(&src, options) {
        parser.handle(event);
    }
    parser.finish()
}

fn heading_class(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "title-1",
        HeadingLevel::H2 => "title-2",
        _ => "title-3",
    }
}

fn escape(text: &str) -> String {
    gtk4::glib::markup_escape_text(text).to_string()
}

struct TableState {
    alignments: Vec<ColumnAlign>,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    header_rows: usize,
}

impl TableState {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments: alignments.into_iter().map(ColumnAlign::from).collect(),
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            header_rows: 0,
        }
    }

    fn push_event(&mut self, event: MdEvent<'_>) {
        match event {
            MdEvent::Start(Tag::TableRow) => self.current_row.clear(),
            MdEvent::End(TagEnd::TableRow) => self.rows.push(std::mem::take(&mut self.current_row)),
            MdEvent::Start(Tag::TableCell) => self.current_cell.clear(),
            MdEvent::End(TagEnd::TableCell) => {
                self.current_row.push(self.current_cell.trim().to_string());
            },
            // Header cells arrive inside TableHead without TableRow
            // events; the head end is what closes the header row.
            MdEvent::End(TagEnd::TableHead) => {
                self.rows.push(std::mem::take(&mut self.current_row));
                self.header_rows = self.rows.len();
            },
            MdEvent::Text(text) | MdEvent::Code(text) => self.current_cell.push_str(&text),
            MdEvent::SoftBreak | MdEvent::HardBreak => self.current_cell.push(' '),
            _ => {},
        }
    }

    fn into_block(self) -> Block {
        Block::Table {
            alignments: self.alignments,
            header_rows: self.header_rows,
            rows: self.rows,
        }
    }
}

// --- widgets ------------------------------------------------------------

fn build_block(block: &Block) -> Widget {
    match block {
        Block::Paragraph { markup } => build_text(markup),
        Block::Heading { level, markup } => build_heading(*level, markup),
        Block::Quote(children) => build_quote(children),
        Block::List(list) => build_list(list),
        Block::Code { language, code } => build_code(language, code),
        Block::Table {
            alignments,
            header_rows,
            rows,
        } => build_table(alignments, *header_rows, rows),
        Block::Rule => Separator::new(Orientation::Horizontal).upcast(),
    }
}

/// A vertical box of child blocks with `gap_above` rhythm. The first child's gap
/// is 0, so it never stacks on a list's `row_spacing` or a quote's padding.
fn build_blocks(blocks: &[Block]) -> GtkBox {
    let container = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(0)
        .build();
    let mut prev: Option<&Block> = None;
    for block in blocks {
        let widget = build_block(block);
        widget.set_margin_top(gap_above(prev, block));
        container.append(&widget);
        prev = Some(block);
    }
    container
}

/// Inter-block top margin, by block kind. Markdown wants asymmetric rhythm: a
/// heading binds to the paragraph below it but gets a larger gap above.
fn gap_above(prev: Option<&Block>, next: &Block) -> i32 {
    if prev.is_none() {
        return 0;
    }
    match next {
        Block::Heading { .. } => 16,
        Block::Paragraph { .. } if matches!(prev, Some(Block::Heading { .. })) => 4,
        _ => 8,
    }
}

fn build_text(markup: &str) -> Widget {
    let label = base_label(markup);
    label.set_attributes(Some(&prose_attrs()));
    label.add_css_class("scry-md-text");
    label.add_css_class("scry-chat-text");
    label.upcast()
}

/// ~1.3 line spacing for prose. GTK4 CSS doesn't honor `line-height` on labels, so
/// this rides on a Pango line-height attribute; the inline `use_markup` attributes
/// (bold/italic/links) still apply on top.
fn prose_attrs() -> pango::AttrList {
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFloat::new_line_height(1.3));
    attrs
}

/// Headings reuse libadwaita's `.title-N` sizes; our app-priority base font rules
/// would override them, so headings skip `.scry-chat-text`. `.scry-md-heading`
/// carries the selection styling instead (see style.css).
fn build_heading(level: HeadingLevel, markup: &str) -> Widget {
    let label = base_label(markup);
    label.add_css_class(heading_class(level));
    label.add_css_class("scry-md-heading");
    label.upcast()
}

/// Selectable, wrapping markup label. The width hint keeps selectable+wrapping
/// labels from collapsing.
fn base_label(markup: &str) -> Label {
    Label::builder()
        .use_markup(true)
        .label(markup)
        .xalign(0.0)
        .halign(Align::Fill)
        .hexpand(true)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .selectable(true)
        .width_chars(1)
        .build()
}

fn build_quote(children: &[Block]) -> Widget {
    let container = build_blocks(children);
    container.add_css_class("scry-md-quote");
    container.upcast()
}

/// A list level: a 2-column grid of marker | content. The grid auto-sizes the
/// marker column to the widest marker, so ordinals (incl. "10.") align, and the
/// content column wraps in the remaining width — a true hanging indent.
fn build_list(list: &ListBlock) -> Widget {
    let grid = Grid::builder().column_spacing(8).row_spacing(6).build();
    for (row, item) in list.items.iter().enumerate() {
        grid.attach(&build_marker(item), 0, row as i32, 1, 1);
        let content = build_blocks(&item.children);
        content.set_hexpand(true);
        grid.attach(&content, 1, row as i32, 1, 1);
    }
    grid.upcast()
}

/// Task items render a read-only checkbox in the marker column instead of the
/// bullet/ordinal — never both. The `CheckButton` stays `sensitive` (so it isn't
/// dimmed) but can't take pointer/keyboard input, so it reads as content.
fn build_marker(item: &ListItem) -> Widget {
    if let Some(checked) = item.task {
        CheckButton::builder()
            .active(checked)
            .can_target(false)
            .can_focus(false)
            .valign(Align::Start)
            .build()
            .upcast()
    } else {
        let label = Label::builder()
            .label(&item.marker)
            .halign(Align::End)
            .valign(Align::Start)
            .build();
        label.add_css_class("scry-md-marker");
        label.upcast()
    }
}

/// Code card: the fenced language (or "code") captions the shared copyable card.
fn build_code(language: &str, code: &str) -> Widget {
    let caption = if language.is_empty() {
        "code"
    } else {
        language
    };
    super::code_card(caption, code).upcast()
}

fn build_table(alignments: &[ColumnAlign], header_rows: usize, rows: &[Vec<String>]) -> Widget {
    let grid = Grid::builder().row_spacing(0).column_spacing(16).build();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(1) as i32;

    let mut grid_row = 0;
    for (row_idx, row) in rows.iter().enumerate() {
        for (column, cell) in row.iter().enumerate() {
            let xalign = match alignments.get(column) {
                Some(ColumnAlign::Center) => 0.5,
                Some(ColumnAlign::Right) => 1.0,
                _ => 0.0,
            };
            let label = Label::builder()
                .label(cell)
                .xalign(xalign)
                .wrap(true)
                .wrap_mode(pango::WrapMode::WordChar)
                .selectable(true)
                .css_classes(if row_idx < header_rows {
                    &["scry-md-th", "scry-md-td", "scry-chat-text"][..]
                } else {
                    &["scry-md-td", "scry-chat-text"][..]
                })
                .build();
            grid.attach(&label, column as i32, grid_row, 1, 1);
        }
        grid_row += 1;

        // A thin horizontal rule under each row (no vertical lines), stronger under
        // the last header row. Spanning all columns keeps the line continuous across
        // the column gaps.
        let rule = GtkBox::new(Orientation::Horizontal, 0);
        rule.add_css_class("scry-md-table-rule");
        if row_idx + 1 == header_rows {
            rule.add_css_class("scry-md-table-rule-head");
        }
        grid.attach(&rule, 0, grid_row, columns, 1);
        grid_row += 1;
    }

    grid.upcast()
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
    use super::*;

    fn paragraph(markup: &str) -> Block {
        Block::Paragraph {
            markup: markup.to_string(),
        }
    }

    #[test]
    fn unwraps_markdown_fenced_table() {
        let src = "before\n```markdown\n| A | B |\n| --- | --- |\n| 1 | 2 |\n```\nafter";

        assert_eq!(
            unwrap_markdown_table_fences(src),
            "before\n| A | B |\n| --- | --- |\n| 1 | 2 |\nafter"
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

        assert_eq!(blocks, vec![paragraph("hello <b>bold</b> world")]);
    }

    #[test]
    fn escapes_markup_characters_in_text() {
        let blocks = parse_blocks("a < b & c");

        assert_eq!(blocks, vec![paragraph("a &lt; b &amp; c")]);
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
                alignments: vec![ColumnAlign::Left, ColumnAlign::Left],
                header_rows: 1,
                rows: vec![
                    vec!["A".to_string(), "B".to_string()],
                    vec!["1".to_string(), "2".to_string()],
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
                markup: "Title".to_string(),
            }]
        );
    }

    #[test]
    fn parses_tight_bullet_list() {
        let blocks = parse_blocks("- one\n- two");

        assert_eq!(
            blocks,
            vec![Block::List(ListBlock {
                ordered: false,
                items: vec![
                    ListItem {
                        marker: "•".to_string(),
                        task: None,
                        children: vec![paragraph("one")],
                    },
                    ListItem {
                        marker: "•".to_string(),
                        task: None,
                        children: vec![paragraph("two")],
                    },
                ],
            })]
        );
    }

    #[test]
    fn ordered_list_numbers_into_double_digits() {
        let blocks = parse_blocks("9. a\n10. b\n11. c");

        let Block::List(list) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        assert!(list.ordered);
        let markers: Vec<&str> = list.items.iter().map(|i| i.marker.as_str()).collect();
        assert_eq!(markers, ["9.", "10.", "11."]);
    }

    #[test]
    fn task_list_item_checked() {
        let blocks = parse_blocks("- [x] done");

        let Block::List(list) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        assert_eq!(list.items[0].task, Some(true));
    }

    #[test]
    fn task_list_item_unchecked() {
        let blocks = parse_blocks("- [ ] todo");

        let Block::List(list) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        assert_eq!(list.items[0].task, Some(false));
    }

    #[test]
    fn nested_tight_list_lives_in_parent_item() {
        let blocks = parse_blocks("- a\n  - b");

        let Block::List(outer) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        let children = &outer.items[0].children;
        assert_eq!(children[0], paragraph("a"));
        let Block::List(inner) = &children[1] else {
            panic!("expected nested list, got {children:?}");
        };
        assert_eq!(inner.items[0].children, vec![paragraph("b")]);
    }

    #[test]
    fn loose_item_keeps_multiple_paragraphs() {
        let blocks = parse_blocks("- one\n\n  two");

        let Block::List(list) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        assert_eq!(
            list.items[0].children,
            vec![paragraph("one"), paragraph("two")]
        );
    }

    #[test]
    fn blockquote_with_list_unwinds_at_eof() {
        // No trailing newline: the Quote and its inner List are both still open at
        // end of input, so finish() must unwind both (the v14 streaming-EOF case).
        let blocks = parse_blocks("> - a\n> - b");

        assert_eq!(blocks.len(), 1);
        let Block::Quote(children) = &blocks[0] else {
            panic!("expected quote, got {blocks:?}");
        };
        let Block::List(list) = &children[0] else {
            panic!("expected list in quote, got {children:?}");
        };
        assert_eq!(list.items.len(), 2);
    }

    #[test]
    fn code_block_inside_list_item_attaches_to_item() {
        let blocks = parse_blocks("- ```rust\n  fn f() {}\n  ```");

        // The code block must land in the item, not leak to the document root.
        assert_eq!(blocks.len(), 1);
        let Block::List(list) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        assert!(matches!(
            list.items[0].children.last(),
            Some(Block::Code { .. })
        ));
    }

    #[test]
    fn unclosed_list_item_while_streaming_still_renders() {
        let blocks = parse_blocks("- one");

        let Block::List(list) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        assert_eq!(list.items[0].children, vec![paragraph("one")]);
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
                    markup: "a".to_string()
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
        let Block::List(list) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        assert!(
            list.items[0]
                .children
                .iter()
                .any(|b| matches!(b, Block::Table { .. })),
            "table should attach to the item: {:?}",
            list.items[0].children
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
        // Three levels, no trailing newline: finish() must unwind every frame.
        let blocks = parse_blocks("- a\n  - b\n    - c");

        let nested = |children: &[Block]| -> ListBlock {
            for child in children {
                if let Block::List(list) = child {
                    return list.clone();
                }
            }
            panic!("expected a nested list in {children:?}");
        };

        let Block::List(l1) = &blocks[0] else {
            panic!("expected list, got {blocks:?}");
        };
        let l2 = nested(&l1.items[0].children);
        let l3 = nested(&l2.items[0].children);
        assert_eq!(l3.items[0].children, vec![paragraph("c")]);
    }

    #[test]
    fn parsed_markdown_reports_node_counts() {
        let parsed = ParsedMarkdown::parse("- a\n- b");

        // One top-level List; node_count = list(1) + 2×(item(1) + paragraph(1)).
        assert_eq!(parsed.top_level_blocks(), 1);
        assert_eq!(parsed.node_count(), 5);
    }
}
