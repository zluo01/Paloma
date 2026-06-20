//! Widget-per-block markdown rendering for assistant output.
//!
//! Renders a parsed [`Block`] tree (produced by the sibling [`parser`](super::parser)
//! module) to one widget per top-level block. Inline nodes (text, emphasis, links,
//! code spans) are walked into a Pango markup string at build time. [`MarkdownView`]
//! keeps one widget per top-level block and, on each streaming update, rebuilds only
//! from the first changed block, so completed blocks and selections stay untouched.
//!
//! Rendering is split at the [`ParsedMarkdown`] boundary: a pure parse step
//! (`parser::ParsedMarkdown::parse` — no GTK objects, owned `Send` output) and the
//! GTK apply step ([`MarkdownView::apply_parsed`]). The streaming path parses while
//! holding only a brief source borrow, then applies after it is dropped.

use gtk4::{
    Align, Box as GtkBox, CheckButton, Grid, Label, Orientation, Separator, Widget, glib, pango,
    prelude::*,
};
use pulldown_cmark::{Alignment, HeadingLevel};

use super::parser::Block;
pub(super) use super::parser::ParsedMarkdown;

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

fn heading_class(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "title-1",
        HeadingLevel::H2 => "title-2",
        _ => "title-3",
    }
}

// --- inline markup ------------------------------------------------------

/// Walk an inline node run into a Pango markup string.
fn inline_markup(nodes: &[Block]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Block::Text(t) => out.push_str(&escape(t)),
            Block::Strong(c) => wrap(&mut out, "<b>", c, "</b>"),
            Block::Emphasis(c) => wrap(&mut out, "<i>", c, "</i>"),
            Block::Strikethrough(c) => wrap(&mut out, "<s>", c, "</s>"),
            Block::Link { url, children } => {
                out.push_str(&format!("<a href=\"{}\">", escape(url)));
                out.push_str(&inline_markup(children));
                out.push_str("</a>");
            },
            Block::InlineCode(t) => {
                out.push_str(INLINE_CODE_OPEN);
                out.push_str(&escape(t));
                out.push_str(INLINE_CODE_CLOSE);
            },
            Block::SoftBreak | Block::HardBreak => out.push('\n'),
            _ => {},
        }
    }
    out
}

fn wrap(out: &mut String, open: &str, children: &[Block], close: &str) {
    out.push_str(open);
    out.push_str(&inline_markup(children));
    out.push_str(close);
}

fn escape(text: &str) -> String {
    glib::markup_escape_text(text).to_string()
}

// --- widgets ------------------------------------------------------------

fn build_block(block: &Block) -> Widget {
    match block {
        Block::Paragraph(children) => build_text(children),
        Block::Heading { level, children } => build_heading(*level, children),
        Block::Quote(children) => build_quote(children),
        Block::OrderedList { start, items } => build_list(Some(*start), items),
        Block::BulletList(items) | Block::TaskList(items) => build_list(None, items),
        Block::Code { language, code } => build_code(language, code),
        Block::Table { alignments, rows } => build_table(alignments, rows),
        Block::Rule => Separator::new(Orientation::Horizontal).upcast(),
        // Inline nodes only appear inside paragraphs/headings; render any stray
        // one as its own markup line rather than dropping it.
        other => build_text(std::slice::from_ref(other)),
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
        Block::Paragraph(_) if matches!(prev, Some(Block::Heading { .. })) => 4,
        _ => 8,
    }
}

fn build_text(children: &[Block]) -> Widget {
    let label = base_label(&inline_markup(children));
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
fn build_heading(level: HeadingLevel, children: &[Block]) -> Widget {
    let label = base_label(&inline_markup(children));
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
fn build_list(start: Option<u64>, items: &[Block]) -> Widget {
    let grid = Grid::builder().column_spacing(8).row_spacing(6).build();
    for (row, item) in items.iter().enumerate() {
        let (marker, children) = match item {
            Block::Item(children) => {
                let marker = match start {
                    Some(s) => marker_label(&format!("{}.", s + row as u64)),
                    None => marker_label("•"),
                };
                (marker, children)
            },
            Block::TaskItem { checked, children } => (checkbox(*checked), children),
            _ => continue,
        };
        attach_item(&grid, row as i32, marker, children);
    }
    grid.upcast()
}

/// Attach one list row: a marker in column 0, the item's content in column 1.
fn attach_item(grid: &Grid, row: i32, marker: Widget, children: &[Block]) {
    grid.attach(&marker, 0, row, 1, 1);
    let content = build_blocks(children);
    content.set_hexpand(true);
    grid.attach(&content, 1, row, 1, 1);
}

/// Bullet or ordinal marker, right-aligned in the marker column.
fn marker_label(text: &str) -> Widget {
    let label = Label::builder()
        .label(text)
        .halign(Align::End)
        .valign(Align::Start)
        .build();
    label.add_css_class("scry-md-marker");
    label.upcast()
}

/// Read-only checkbox for a task item. Stays `sensitive` (so it isn't dimmed) but
/// can't take pointer/keyboard input, so it reads as content.
fn checkbox(checked: bool) -> Widget {
    CheckButton::builder()
        .active(checked)
        .can_target(false)
        .can_focus(false)
        .valign(Align::Start)
        .build()
        .upcast()
}

/// Code card: the fenced language (or "code") captions the shared copyable card.
fn build_code(language: &str, code: &str) -> Widget {
    let caption = if language.is_empty() {
        "code"
    } else {
        language
    };
    super::helper::code_card(caption, code).upcast()
}

fn build_table(alignments: &[Alignment], rows: &[Vec<String>]) -> Widget {
    let grid = Grid::builder().row_spacing(0).column_spacing(16).build();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(1) as i32;

    let mut grid_row = 0;
    for (row_idx, row) in rows.iter().enumerate() {
        for (column, cell) in row.iter().enumerate() {
            let xalign = match alignments.get(column) {
                Some(Alignment::Center) => 0.5,
                Some(Alignment::Right) => 1.0,
                _ => 0.0,
            };
            let label = Label::builder()
                .label(cell)
                .xalign(xalign)
                .wrap(true)
                .wrap_mode(pango::WrapMode::WordChar)
                .selectable(true)
                .css_classes(if row_idx == 0 {
                    &["scry-md-th", "scry-md-td", "scry-chat-text"][..]
                } else {
                    &["scry-md-td", "scry-chat-text"][..]
                })
                .build();
            grid.attach(&label, column as i32, grid_row, 1, 1);
        }
        grid_row += 1;

        // A thin horizontal rule under each row (no vertical lines), stronger under
        // the header row. Spanning all columns keeps the line continuous across the
        // column gaps.
        let rule = GtkBox::new(Orientation::Horizontal, 0);
        rule.add_css_class("scry-md-table-rule");
        if row_idx == 0 {
            rule.add_css_class("scry-md-table-rule-head");
        }
        grid.attach(&rule, 0, grid_row, columns, 1);
        grid_row += 1;
    }

    grid.upcast()
}
