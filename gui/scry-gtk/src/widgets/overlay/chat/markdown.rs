//! Widget-per-block markdown rendering for assistant output.
//!
//! The accumulated source is parsed into a flat list of [`Block`]s;
//! [`MarkdownView`] keeps one widget per block and, on each streaming
//! update, rebuilds only from the first changed block. During append-streaming
//! that is usually just the last block, so completed blocks and selections stay
//! untouched.

use std::borrow::Cow;

use gtk4::{Align, Box as GtkBox, Grid, Label, Orientation, Separator, Widget, pango, prelude::*};
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
    /// Paragraphs, headings, lists, and quotes as Pango markup plus a CSS class.
    Text {
        markup: String,
        class: Option<&'static str>,
    },
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
        let widget = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(8)
            .build();
        Self {
            widget,
            blocks: Vec::new(),
        }
    }

    /// Re-render for the full accumulated `src`, rebuilding widgets only
    /// from the first block that changed.
    pub(super) fn set_markdown(&mut self, src: &str) {
        let next = parse_blocks(src);
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
            self.widget.append(&widget);
            self.blocks.push(Rendered { block, widget });
        }
    }
}

// --- parsing ------------------------------------------------------------

/// Inline accumulation for the current text block.
#[derive(Default)]
struct Inline {
    markup: String,
    heading: Option<&'static str>,
    quote_depth: usize,
}

impl Inline {
    fn flush(&mut self, blocks: &mut Vec<Block>) {
        let markup = std::mem::take(&mut self.markup);
        let heading = self.heading.take();
        let markup = markup.trim_matches('\n');
        if markup.trim().is_empty() {
            return;
        }
        let class = heading.or((self.quote_depth > 0).then_some("scry-md-quote"));
        blocks.push(Block::Text {
            markup: markup.to_string(),
            class,
        });
    }
}

fn parse_blocks(src: &str) -> Vec<Block> {
    let src = unwrap_markdown_table_fences(src);
    let mut blocks = Vec::new();
    let mut inline = Inline::default();
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut code: Option<(String, String)> = None;
    let mut table: Option<TableState> = None;

    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(&src, options) {
        match event {
            MdEvent::Start(Tag::Table(alignments)) => {
                inline.flush(&mut blocks);
                table = Some(TableState::new(alignments));
            },
            MdEvent::End(TagEnd::Table) => {
                if let Some(table) = table.take() {
                    blocks.push(table.into_block());
                }
            },
            event if table.is_some() => {
                if let Some(table) = table.as_mut() {
                    table.push_event(event);
                }
            },

            MdEvent::Start(Tag::CodeBlock(kind)) => {
                inline.flush(&mut blocks);
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    },
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((language, String::new()));
            },
            MdEvent::Text(text) if code.is_some() => {
                if let Some((_, body)) = code.as_mut() {
                    body.push_str(&text);
                }
            },
            MdEvent::End(TagEnd::CodeBlock) => {
                if let Some((language, body)) = code.take() {
                    blocks.push(Block::Code {
                        language,
                        code: body.trim_end().to_string(),
                    });
                }
            },

            MdEvent::Start(Tag::Paragraph) => {},
            MdEvent::End(TagEnd::Paragraph) => {
                // Inside a list, paragraphs are item content; the block
                // flushes when the outermost list ends.
                if lists.is_empty() {
                    inline.flush(&mut blocks);
                } else {
                    inline.markup.push('\n');
                }
            },

            MdEvent::Start(Tag::Heading { level, .. }) => {
                inline.flush(&mut blocks);
                inline.heading = Some(heading_class(level));
            },
            MdEvent::End(TagEnd::Heading(_)) => inline.flush(&mut blocks),

            MdEvent::Start(Tag::BlockQuote(_)) => {
                inline.flush(&mut blocks);
                inline.quote_depth += 1;
            },
            MdEvent::End(TagEnd::BlockQuote(_)) => {
                inline.flush(&mut blocks);
                inline.quote_depth = inline.quote_depth.saturating_sub(1);
            },

            MdEvent::Start(Tag::List(start)) => {
                if lists.is_empty() {
                    inline.flush(&mut blocks);
                }
                lists.push(start);
            },
            MdEvent::End(TagEnd::List(_)) => {
                lists.pop();
                if lists.is_empty() {
                    inline.flush(&mut blocks);
                }
            },
            MdEvent::Start(Tag::Item) => {
                if !inline.markup.is_empty() && !inline.markup.ends_with('\n') {
                    inline.markup.push('\n');
                }
                inline
                    .markup
                    .push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                let marker = match lists.last_mut() {
                    Some(Some(n)) => {
                        let marker = format!("{n}. ");
                        *n += 1;
                        marker
                    },
                    _ => "• ".to_string(),
                };
                inline.markup.push_str(&marker);
            },
            MdEvent::End(TagEnd::Item) if !inline.markup.ends_with('\n') => {
                inline.markup.push('\n');
            },

            MdEvent::Start(Tag::Strong) => inline.markup.push_str("<b>"),
            MdEvent::End(TagEnd::Strong) => inline.markup.push_str("</b>"),
            MdEvent::Start(Tag::Emphasis) => inline.markup.push_str("<i>"),
            MdEvent::End(TagEnd::Emphasis) => inline.markup.push_str("</i>"),
            MdEvent::Start(Tag::Strikethrough) => inline.markup.push_str("<s>"),
            MdEvent::End(TagEnd::Strikethrough) => inline.markup.push_str("</s>"),
            MdEvent::Start(Tag::Link { dest_url, .. }) => {
                inline
                    .markup
                    .push_str(&format!("<a href=\"{}\">", escape(&dest_url)));
            },
            MdEvent::End(TagEnd::Link) => inline.markup.push_str("</a>"),
            MdEvent::Code(text) => {
                inline.markup.push_str(INLINE_CODE_OPEN);
                inline.markup.push_str(&escape(&text));
                inline.markup.push_str(INLINE_CODE_CLOSE);
            },
            MdEvent::Text(text) => inline.markup.push_str(&escape(&text)),
            MdEvent::SoftBreak | MdEvent::HardBreak => inline.markup.push('\n'),
            MdEvent::Rule => {
                inline.flush(&mut blocks);
                blocks.push(Block::Rule);
            },

            _ => {},
        }
    }

    // Streaming may end mid-construct; flush whatever is open.
    inline.flush(&mut blocks);
    if let Some((language, body)) = code.take() {
        blocks.push(Block::Code {
            language,
            code: body.trim_end().to_string(),
        });
    }
    if let Some(table) = table.take() {
        blocks.push(table.into_block());
    }
    blocks
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
        Block::Text { markup, class } => build_text(markup, *class),
        Block::Code { language, code } => build_code(language, code),
        Block::Table {
            alignments,
            header_rows,
            rows,
        } => build_table(alignments, *header_rows, rows),
        Block::Rule => Separator::new(Orientation::Horizontal).upcast(),
    }
}

fn build_text(markup: &str, class: Option<&'static str>) -> Widget {
    let label = Label::builder()
        .use_markup(true)
        .label(markup)
        .xalign(0.0)
        .halign(Align::Fill)
        .hexpand(true)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .selectable(true)
        // Width hint: selectable+wrapping labels collapse without one.
        .width_chars(1)
        .build();
    match class {
        // Headings reuse libadwaita's .title-N sizes; our app-priority base
        // font rules would override them, so headings skip those classes.
        Some(title @ ("title-1" | "title-2" | "title-3")) => label.add_css_class(title),
        other => {
            label.add_css_class("scry-md-text");
            label.add_css_class("scry-chat-text");
            if let Some(class) = other {
                label.add_css_class(class);
            }
        },
    }
    label.upcast()
}

/// Code card: the fenced language (or "code") captions the shared
/// copyable card.
fn build_code(language: &str, code: &str) -> Widget {
    let caption = if language.is_empty() {
        "code"
    } else {
        language
    };
    super::code_card(caption, code).upcast()
}

fn build_table(alignments: &[ColumnAlign], header_rows: usize, rows: &[Vec<String>]) -> Widget {
    let grid = Grid::builder().row_spacing(4).column_spacing(16).build();

    // Body rows shift down one when a separator follows the header.
    let separator_row = (header_rows > 0).then_some(header_rows);
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);

    for (row_idx, row) in rows.iter().enumerate() {
        let grid_row = match separator_row {
            Some(at) if row_idx >= at => row_idx + 1,
            _ => row_idx,
        };
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
                    &["scry-md-th", "scry-chat-text"][..]
                } else {
                    &["scry-chat-text"][..]
                })
                .build();
            grid.attach(&label, column as i32, grid_row as i32, 1, 1);
        }
    }

    if let Some(at) = separator_row {
        let separator = Separator::new(Orientation::Horizontal);
        grid.attach(&separator, 0, at as i32, columns.max(1) as i32, 1);
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

        assert_eq!(blocks.len(), 1);
        let Block::Text { markup, class } = &blocks[0] else {
            panic!("expected text block, got {blocks:?}");
        };
        assert_eq!(markup, "hello <b>bold</b> world");
        assert_eq!(*class, None);
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
    fn escapes_markup_characters_in_text() {
        let blocks = parse_blocks("a < b & c");

        let Block::Text { markup, .. } = &blocks[0] else {
            panic!("expected text block, got {blocks:?}");
        };
        assert_eq!(markup, "a &lt; b &amp; c");
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
    fn heading_gets_level_class() {
        let blocks = parse_blocks("## Title");

        let Block::Text { class, .. } = &blocks[0] else {
            panic!("expected text block, got {blocks:?}");
        };
        assert_eq!(*class, Some("title-2"));
    }

    #[test]
    fn list_items_get_markers() {
        let blocks = parse_blocks("- one\n- two");

        let Block::Text { markup, .. } = &blocks[0] else {
            panic!("expected text block, got {blocks:?}");
        };
        assert_eq!(markup, "• one\n• two");
    }
}
