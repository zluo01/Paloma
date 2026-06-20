//! Parsed markdown node definitions — the single tree the renderer consumes.
//!
//! Everything is a [`Block`]: containers hold child blocks, leaves hold text or
//! rows, and inline runs (text, emphasis, links) are blocks too. The parser
//! builds this tree directly on a stack as it walks the event stream.

use pulldown_cmark::{Alignment, HeadingLevel};

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Block {
    // --- containers ---
    Quote(Vec<Block>),
    OrderedList {
        start: u64,
        items: Vec<Block>,
    },
    BulletList(Vec<Block>),
    TaskList(Vec<Block>),
    Item(Vec<Block>),
    TaskItem {
        checked: bool,
        children: Vec<Block>,
    },
    // --- leaf blocks ---
    Paragraph(Vec<Block>),
    Heading {
        level: HeadingLevel,
        children: Vec<Block>,
    },
    Code {
        language: String,
        code: String,
    },
    Table {
        alignments: Vec<Alignment>,
        rows: Vec<Vec<String>>,
    },
    Rule,
    // --- inline ---
    Text(String),
    Strong(Vec<Block>),
    Emphasis(Vec<Block>),
    Strikethrough(Vec<Block>),
    Link {
        url: String,
        children: Vec<Block>,
    },
    InlineCode(String),
    SoftBreak,
    HardBreak,
}

impl Block {
    pub(crate) fn children_mut(&mut self) -> Option<&mut Vec<Block>> {
        match self {
            Block::Quote(c)
            | Block::BulletList(c)
            | Block::TaskList(c)
            | Block::Item(c)
            | Block::Paragraph(c)
            | Block::Strong(c)
            | Block::Emphasis(c)
            | Block::Strikethrough(c) => Some(c),
            Block::OrderedList { items, .. } => Some(items),
            Block::TaskItem { children, .. }
            | Block::Heading { children, .. }
            | Block::Link { children, .. } => Some(children),
            _ => None,
        }
    }

    pub(crate) fn children(&self) -> &[Block] {
        match self {
            Block::Quote(c)
            | Block::BulletList(c)
            | Block::TaskList(c)
            | Block::Item(c)
            | Block::Paragraph(c)
            | Block::Strong(c)
            | Block::Emphasis(c)
            | Block::Strikethrough(c) => c,
            Block::OrderedList { items, .. } => items,
            Block::TaskItem { children, .. }
            | Block::Heading { children, .. }
            | Block::Link { children, .. } => children,
            _ => &[],
        }
    }
}
