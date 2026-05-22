/// A semantic element produced by the classification pipeline from a parsed
/// `RawBody`. This is the layer exposed to consumers — runs, hyperlinks, and
/// numbering bookkeeping have already been resolved into human-meaningful
/// blocks.
use crate::numbering::ListType;

/// Lightweight reference to the most recent heading above a block, used to
/// give downstream consumers (RAG, exporters) the section context they need
/// without forcing them to walk the element list themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingContext {
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocxElement {
    /// A regular text paragraph.
    Paragraph {
        text: String,
        parent_heading: Option<HeadingContext>,
    },
    /// A heading. `level` is 1..=9 (Word's outline levels).
    Heading { level: u8, text: String },
    /// A list item resolved against the numbering tables.
    /// `display_index` is `None` for bullets and unsupported formats.
    ListItem {
        text: String,
        level: u8,
        list_type: ListType,
        display_index: Option<u32>,
    },
}
