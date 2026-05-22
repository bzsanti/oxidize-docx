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

/// A single cell of a `Table` element. `col_span` and `row_span` reflect
/// resolved OOXML `w:gridSpan` and `w:vMerge` semantics — cells absorbed
/// into a vertical merge are NOT emitted; their span is carried by the
/// anchoring (Restart) cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    pub text: String,
    pub col_span: u16,
    pub row_span: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
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
    /// A table with spans resolved. Cells absorbed into vMerge runs are
    /// omitted; their row_span is folded into the anchoring cell.
    Table { rows: Vec<TableRow> },
    /// A footnote resolved against `word/footnotes.xml`. Emitted directly
    /// after the paragraph that contains its `<w:footnoteReference w:id>`.
    Footnote { id: u32, text: String },
    /// An endnote resolved against `word/endnotes.xml`. Emitted directly
    /// after the paragraph that contains its `<w:endnoteReference w:id>`.
    Endnote { id: u32, text: String },
    /// A comment (review annotation) resolved against `word/comments.xml`.
    /// Emitted directly after the paragraph that contains its
    /// `<w:commentReference w:id>`, carrying both the reviewer's name
    /// and the comment body so downstream consumers can route them
    /// independently of the surrounding prose.
    Comment {
        id: u32,
        author: String,
        text: String,
    },
}
