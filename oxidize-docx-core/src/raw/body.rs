use super::paragraphs::RawParagraph;
use super::tables::RawTable;

/// Kind of a `<w:headerReference>` or `<w:footerReference>` slot:
/// the section may declare a different header/footer for the first
/// page, the even pages, or use the default for everything else.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum SectionRefType {
    Default,
    First,
    Even,
}

/// A single reference from a `<w:sectPr>` to a header or footer part.
/// `rel_id` resolves against `word/_rels/document.xml.rels`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawSectionRef {
    pub(crate) rel_id: String,
    pub(crate) ref_type: SectionRefType,
}

/// Properties carried by a `<w:sectPr>` element. For now we only capture
/// the references the classifier needs to resolve header/footer parts.
/// Page size, columns, margins, etc. are intentionally not modelled —
/// they don't drive the semantic element pipeline.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct RawSectionProperties {
    pub(crate) header_refs: Vec<RawSectionRef>,
    pub(crate) footer_refs: Vec<RawSectionRef>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum RawBodyItem {
    Paragraph(RawParagraph),
    Table(RawTable),
    SectionBreak(RawSectionProperties),
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct RawBody {
    pub(crate) items: Vec<RawBodyItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::tables::{RawTable, RawTableProperties, RawTableRow};

    #[test]
    fn raw_body_item_paragraph_variant() {
        let item = RawBodyItem::Paragraph(RawParagraph::default());
        assert!(matches!(item, RawBodyItem::Paragraph(_)));
    }

    #[test]
    fn raw_body_item_table_variant() {
        let item = RawBodyItem::Table(RawTable {
            properties: RawTableProperties::default(),
            rows: vec![],
        });
        assert!(matches!(item, RawBodyItem::Table(_)));
    }

    #[test]
    fn raw_body_item_section_break_variant() {
        let item = RawBodyItem::SectionBreak(RawSectionProperties::default());
        assert!(matches!(item, RawBodyItem::SectionBreak(_)));
    }

    #[test]
    fn raw_body_mixed_items() {
        let body = RawBody {
            items: vec![
                RawBodyItem::Paragraph(RawParagraph::default()),
                RawBodyItem::Table(RawTable {
                    properties: RawTableProperties::default(),
                    rows: vec![RawTableRow { cells: vec![] }],
                }),
                RawBodyItem::Paragraph(RawParagraph::default()),
                RawBodyItem::SectionBreak(RawSectionProperties::default()),
            ],
        };
        assert_eq!(body.items.len(), 4);
        assert!(matches!(body.items[0], RawBodyItem::Paragraph(_)));
        assert!(matches!(body.items[1], RawBodyItem::Table(_)));
        assert!(matches!(body.items[3], RawBodyItem::SectionBreak(_)));
    }
}
