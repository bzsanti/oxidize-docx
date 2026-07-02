use std::collections::HashMap;

use crate::pipeline::element::{TableCell, TableRow};
use crate::raw::tables::{RawTable, RawTableCell, RawVMerge};

/// Resolves OOXML span semantics on a `RawTable` and emits a flat
/// `Vec<TableRow>` with `col_span` (from `w:gridSpan`) and `row_span`
/// (from `w:vMerge`) collapsed onto the anchoring cells. Cells absorbed
/// into a vertical merge are not emitted; their existence is only
/// observable through the anchoring cell's increased `row_span`. As a
/// result, the row immediately below a `vMerge=Restart` may contain
/// fewer cells than its raw counterpart.
pub(crate) fn build_table(raw: &RawTable) -> Vec<TableRow> {
    let mut rows: Vec<TableRow> = Vec::with_capacity(raw.rows.len());

    // Column position (after applying preceding gridSpans) of the most
    // recent vMerge=Restart anchor, mapped to the (row_index, cell_index)
    // it occupies in the output. Continue cells look up by column to find
    // the anchor whose row_span they should grow.
    let mut anchors: HashMap<u32, (usize, usize)> = HashMap::new();

    for raw_row in &raw.rows {
        let row_index = rows.len();
        let mut row_cells: Vec<TableCell> = Vec::new();
        let mut col: u32 = 0;
        for raw_cell in &raw_row.cells {
            let span = raw_cell.properties.grid_span as u32;
            match &raw_cell.properties.v_merge {
                Some(RawVMerge::Continue) => {
                    if let Some(&(r, c)) = anchors.get(&col) {
                        rows[r].cells[c].row_span += 1;
                    }
                }
                Some(RawVMerge::Restart) => {
                    let cell_index = row_cells.len();
                    row_cells.push(TableCell {
                        text: cell_text(raw_cell),
                        col_span: raw_cell.properties.grid_span,
                        row_span: 1,
                    });
                    anchors.insert(col, (row_index, cell_index));
                }
                None => {
                    row_cells.push(TableCell {
                        text: cell_text(raw_cell),
                        col_span: raw_cell.properties.grid_span,
                        row_span: 1,
                    });
                    anchors.remove(&col);
                }
            }
            col += span;
        }
        rows.push(TableRow { cells: row_cells });
    }

    rows
}

fn cell_text(cell: &RawTableCell) -> String {
    use crate::raw::paragraphs::RawInline;
    let mut s = String::new();
    for (i, p) in cell.paragraphs.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        for inline in &p.content {
            if let RawInline::Run(run) = inline {
                if let Some(t) = &run.text {
                    s.push_str(t);
                }
            }
        }
    }
    s
}
