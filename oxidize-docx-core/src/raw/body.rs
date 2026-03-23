use super::paragraphs::RawParagraph;
use super::tables::RawTable;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum RawBodyItem {
    Paragraph(RawParagraph),
    Table(RawTable),
    SectionBreak,
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
        let item = RawBodyItem::SectionBreak;
        assert!(matches!(item, RawBodyItem::SectionBreak));
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
                RawBodyItem::SectionBreak,
            ],
        };
        assert_eq!(body.items.len(), 4);
        assert!(matches!(body.items[0], RawBodyItem::Paragraph(_)));
        assert!(matches!(body.items[1], RawBodyItem::Table(_)));
        assert!(matches!(body.items[3], RawBodyItem::SectionBreak));
    }
}
