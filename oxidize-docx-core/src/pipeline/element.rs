/// A semantic element produced by the classification pipeline from a parsed
/// `RawBody`. This is the layer exposed to consumers — runs, hyperlinks, and
/// numbering bookkeeping have already been resolved into human-meaningful
/// blocks.
use crate::numbering::ListType;
use crate::raw::body::SectionRefType;

/// Which slot of a section's header/footer a `Header` or `Footer` element
/// fills. Default applies to every page that isn't overridden by First
/// (first-page-only) or Even (even-numbered pages, used in mirrored layouts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderKind {
    Default,
    First,
    Even,
}

impl From<&SectionRefType> for HeaderKind {
    fn from(t: &SectionRefType) -> Self {
        match t {
            SectionRefType::Default => HeaderKind::Default,
            SectionRefType::First => HeaderKind::First,
            SectionRefType::Even => HeaderKind::Even,
        }
    }
}

/// Lightweight reference to the most recent heading above a block, used to
/// give downstream consumers (RAG, exporters) the section context they need
/// without forcing them to walk the element list themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingContext {
    pub level: u8,
    pub text: String,
}

/// Metadata for a hyperlink span inside a paragraph. The link's visible
/// text is already part of `Paragraph::text`; this struct carries the
/// URL alongside, in document order, so exporters can re-decorate the
/// matching span (e.g. markdown `[text](url)`) without scanning the
/// raw layer again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSpan {
    pub text: String,
    pub url: String,
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
    /// A regular text paragraph. `links` carries the URL metadata for
    /// hyperlinks that appear inside the paragraph, in document order;
    /// the visible text of each link is already part of `text`.
    Paragraph {
        text: String,
        parent_heading: Option<HeadingContext>,
        links: Vec<LinkSpan>,
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
    /// A hyperlink (external URL or in-document anchor) emitted after
    /// the paragraph that contains it. `url` is the resolved target:
    /// an absolute URL for external links (resolved via the document's
    /// relationships) or `#anchor` for in-document references.
    ///
    /// Note: OOXML interleaves runs and hyperlinks inside a paragraph,
    /// but the raw parser keeps them in two separate vectors and loses
    /// the inline position. Until that is preserved, hyperlinks are
    /// emitted as satellite elements right after their paragraph.
    Hyperlink { text: String, url: String },
    /// A page header. `kind` distinguishes the Default / First-page /
    /// Even-page slot. `content` is the header part fully classified
    /// (paragraphs, tables, etc. — headers can be arbitrarily rich).
    /// Emitted at the position of the `<w:sectPr>` that references it.
    Header {
        kind: HeaderKind,
        content: Vec<DocxElement>,
    },
    /// A page footer. See `Header`.
    Footer {
        kind: HeaderKind,
        content: Vec<DocxElement>,
    },
}
