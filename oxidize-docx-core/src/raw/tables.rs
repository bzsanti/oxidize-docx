use super::paragraphs::RawParagraph;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum RawVMerge {
    Restart,
    Continue,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawTableCellProperties {
    pub(crate) grid_span: u16,
    pub(crate) v_merge: Option<RawVMerge>,
    pub(crate) width: Option<u32>,
}

impl Default for RawTableCellProperties {
    fn default() -> Self {
        Self {
            grid_span: 1,
            v_merge: None,
            width: None,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawTableCell {
    pub(crate) properties: RawTableCellProperties,
    pub(crate) paragraphs: Vec<RawParagraph>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawTableRow {
    pub(crate) cells: Vec<RawTableCell>,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct RawTableProperties {
    pub(crate) style_id: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawTable {
    pub(crate) properties: RawTableProperties,
    pub(crate) rows: Vec<RawTableRow>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_props_default() {
        let cp = RawTableCellProperties::default();
        assert_eq!(cp.grid_span, 1);
        assert!(cp.v_merge.is_none());
        assert!(cp.width.is_none());
    }

    #[test]
    fn cell_contains_paragraphs() {
        let cell = RawTableCell {
            properties: RawTableCellProperties::default(),
            paragraphs: vec![RawParagraph::default()],
        };
        assert_eq!(cell.paragraphs.len(), 1);
    }

    #[test]
    fn row_has_cells() {
        let row = RawTableRow {
            cells: vec![
                RawTableCell {
                    properties: RawTableCellProperties::default(),
                    paragraphs: vec![],
                },
                RawTableCell {
                    properties: RawTableCellProperties::default(),
                    paragraphs: vec![],
                },
            ],
        };
        assert_eq!(row.cells.len(), 2);
    }

    #[test]
    fn table_has_rows() {
        let table = RawTable {
            properties: RawTableProperties::default(),
            rows: vec![RawTableRow {
                cells: vec![RawTableCell {
                    properties: RawTableCellProperties::default(),
                    paragraphs: vec![RawParagraph::default()],
                }],
            }],
        };
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 1);
        assert_eq!(table.rows[0].cells[0].paragraphs.len(), 1);
    }

    #[test]
    fn v_merge_restart() {
        let cp = RawTableCellProperties {
            v_merge: Some(RawVMerge::Restart),
            ..Default::default()
        };
        assert_eq!(cp.v_merge, Some(RawVMerge::Restart));
    }

    #[test]
    fn v_merge_continue() {
        let cp = RawTableCellProperties {
            v_merge: Some(RawVMerge::Continue),
            ..Default::default()
        };
        assert_eq!(cp.v_merge, Some(RawVMerge::Continue));
    }

    #[test]
    fn grid_span_multi_column() {
        let cp = RawTableCellProperties {
            grid_span: 3,
            ..Default::default()
        };
        assert_eq!(cp.grid_span, 3);
    }
}
