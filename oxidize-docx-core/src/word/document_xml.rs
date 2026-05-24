use quick_xml::events::Event;

use crate::error::{DocxError, Result};
use crate::raw::body::{RawBody, RawBodyItem, RawSectionProperties, RawSectionRef, SectionRefType};
use crate::raw::paragraphs::{RawHyperlink, RawInline, RawParagraph};
use crate::raw::runs::RawRun;
use crate::raw::tables::{
    RawTable, RawTableCell, RawTableCellProperties, RawTableProperties, RawTableRow, RawVMerge,
};
use crate::word::styles_xml::{parse_paragraph_properties, parse_run_properties};
use crate::xml::reader::XmlReader;

#[allow(dead_code)]
fn read_w_val(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"w:val" {
            return Some(String::from_utf8_lossy(&attr.value).to_string());
        }
    }
    None
}

#[allow(dead_code)]
fn read_attr(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == key {
            return Some(String::from_utf8_lossy(&attr.value).to_string());
        }
    }
    None
}

/// Parses a `<w:sectPr>` block, capturing its `<w:headerReference>` and
/// `<w:footerReference>` children. Other children (`<w:pgSz>`, margins,
/// columns, line numbering, …) are ignored — they don't affect the
/// semantic element pipeline at this stage.
///
/// Expects the reader positioned just after the `<w:sectPr>` Start event;
/// consumes events up to and including the matching End event.
fn parse_section_properties(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<RawSectionProperties> {
    let mut props = RawSectionProperties::default();
    let mut depth = 1u32;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"w:sectPr" {
                    depth += 1;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.name();
                match local.as_ref() {
                    b"w:headerReference" => {
                        if let Some(r) = parse_section_ref(e) {
                            props.header_refs.push(r);
                        }
                    }
                    b"w:footerReference" => {
                        if let Some(r) = parse_section_ref(e) {
                            props.footer_refs.push(r);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"w:sectPr" {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: "w:sectPr".into(),
                    reason: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(props)
}

fn parse_section_ref(e: &quick_xml::events::BytesStart<'_>) -> Option<RawSectionRef> {
    let rel_id = read_attr(e, b"r:id")?;
    let ref_type = read_attr(e, b"w:type")
        .as_deref()
        .map(parse_section_ref_type)
        .unwrap_or(SectionRefType::Default);
    Some(RawSectionRef { rel_id, ref_type })
}

fn parse_section_ref_type(s: &str) -> SectionRefType {
    match s {
        "first" => SectionRefType::First,
        "even" => SectionRefType::Even,
        _ => SectionRefType::Default,
    }
}

#[allow(dead_code)]
fn skip_element(tag: &[u8], reader: &mut quick_xml::Reader<&[u8]>, buf: &mut Vec<u8>) {
    let mut depth = 1u32;
    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                if name.as_ref() == tag {
                    depth += 1;
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                if name.as_ref() == tag {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
            }
            Ok(Event::Eof) => return,
            Err(_) => return,
            _ => {}
        }
        buf.clear();
    }
}

#[allow(dead_code)]
struct TableState {
    properties: RawTableProperties,
    rows: Vec<RawTableRow>,
    current_row_cells: Vec<RawTableCell>,
    current_cell_paragraphs: Vec<RawParagraph>,
    current_cell_props: RawTableCellProperties,
    in_row: bool,
    in_cell: bool,
    in_cell_props: bool,
}

#[allow(dead_code)]
impl TableState {
    fn new() -> Self {
        Self {
            properties: RawTableProperties::default(),
            rows: Vec::new(),
            current_row_cells: Vec::new(),
            current_cell_paragraphs: Vec::new(),
            current_cell_props: RawTableCellProperties::default(),
            in_row: false,
            in_cell: false,
            in_cell_props: false,
        }
    }

    fn finish_cell(&mut self) {
        if self.in_cell {
            let cell = RawTableCell {
                properties: std::mem::take(&mut self.current_cell_props),
                paragraphs: std::mem::take(&mut self.current_cell_paragraphs),
            };
            self.current_row_cells.push(cell);
            self.in_cell = false;
        }
    }

    fn finish_row(&mut self) {
        self.finish_cell();
        if self.in_row {
            let row = RawTableRow {
                cells: std::mem::take(&mut self.current_row_cells),
            };
            self.rows.push(row);
            self.in_row = false;
        }
    }

    fn into_table(mut self) -> RawTable {
        self.finish_row();
        RawTable {
            properties: self.properties,
            rows: self.rows,
        }
    }
}

/// Parses `word/document.xml` into a `RawBody`. The envelope is the
/// canonical `<w:body>` block.
#[allow(dead_code)]
pub(crate) fn parse_document_xml(xml_bytes: &[u8]) -> Result<RawBody> {
    parse_body_envelope(xml_bytes, b"w:body", "word/document.xml")
}

/// Parses a `word/headerN.xml` part. The envelope is `<w:hdr>` but
/// the inner content (paragraphs, tables, runs, hyperlinks, …) is the
/// same OOXML grammar as `<w:body>`, so the same state machine handles
/// it; only the activating tag changes.
#[allow(dead_code)]
pub(crate) fn parse_header_xml(xml_bytes: &[u8]) -> Result<RawBody> {
    parse_body_envelope(xml_bytes, b"w:hdr", "word/header*.xml")
}

/// Parses a `word/footerN.xml` part. See `parse_header_xml`.
#[allow(dead_code)]
pub(crate) fn parse_footer_xml(xml_bytes: &[u8]) -> Result<RawBody> {
    parse_body_envelope(xml_bytes, b"w:ftr", "word/footer*.xml")
}

fn parse_body_envelope(xml_bytes: &[u8], envelope: &[u8], source: &str) -> Result<RawBody> {
    let mut reader = XmlReader::from_bytes_preserve_text(xml_bytes, source)?;
    let mut body = RawBody::default();
    let mut buf = Vec::new();

    let mut in_body = false;

    // Paragraph state
    let mut in_paragraph = false;
    let mut current_paragraph = RawParagraph::default();

    // Run state
    let mut in_run = false;
    let mut current_run_text: Option<String> = None;
    let mut current_run_properties = crate::raw::runs::RawRunProperties::default();
    let mut in_text = false;

    // Hyperlink state
    let mut in_hyperlink = false;
    let mut current_hyperlink_rel_id: Option<String> = None;
    let mut current_hyperlink_anchor: Option<String> = None;
    let mut hyperlink_runs: Vec<RawRun> = Vec::new();

    // Table state — stack for nested tables
    let mut table_stack: Vec<TableState> = Vec::new();

    loop {
        match reader.inner().read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    n if n == envelope => {
                        in_body = true;
                    }
                    b"w:p" if in_body => {
                        in_paragraph = true;
                        current_paragraph = RawParagraph::default();
                    }
                    b"w:pPr" if in_paragraph => {
                        let ppr = parse_paragraph_properties(reader.inner(), &mut buf)?;
                        current_paragraph.properties = ppr;
                    }
                    b"w:r" if in_paragraph => {
                        in_run = true;
                        current_run_text = None;
                        current_run_properties = crate::raw::runs::RawRunProperties::default();
                    }
                    b"w:rPr" if in_run => {
                        current_run_properties = parse_run_properties(reader.inner(), &mut buf)?;
                    }
                    b"w:t" if in_run => {
                        in_text = true;
                    }
                    b"w:hyperlink" if in_paragraph => {
                        in_hyperlink = true;
                        current_hyperlink_rel_id = read_attr(e, b"r:id");
                        current_hyperlink_anchor = read_attr(e, b"w:anchor");
                        hyperlink_runs.clear();
                    }
                    b"w:tbl" if in_body => {
                        table_stack.push(TableState::new());
                    }
                    b"w:tblPr" if !table_stack.is_empty() => {
                        // Parse table properties inline — extract style, skip the rest
                        let mut tbl_depth = 1u32;
                        loop {
                            match reader.inner().read_event_into(&mut buf) {
                                Ok(Event::Start(ref inner_e)) => {
                                    let inner_name = inner_e.name();
                                    if inner_name.as_ref() == b"w:tblPr" {
                                        tbl_depth += 1;
                                    }
                                }
                                Ok(Event::Empty(ref inner_e)) => {
                                    let inner_name = inner_e.name();
                                    if inner_name.as_ref() == b"w:tblStyle" {
                                        if let Some(ts) = table_stack.last_mut() {
                                            ts.properties.style_id = read_w_val(inner_e);
                                        }
                                    }
                                }
                                Ok(Event::End(ref inner_e)) => {
                                    let inner_name = inner_e.name();
                                    if inner_name.as_ref() == b"w:tblPr" {
                                        tbl_depth -= 1;
                                        if tbl_depth == 0 {
                                            break;
                                        }
                                    }
                                }
                                Ok(Event::Eof) => break,
                                Err(_) => break,
                                _ => {}
                            }
                            buf.clear();
                        }
                    }
                    b"w:tblGrid" if !table_stack.is_empty() => {
                        // Skip grid definition
                        skip_element(b"w:tblGrid", reader.inner(), &mut buf);
                    }
                    b"w:tr" if !table_stack.is_empty() => {
                        if let Some(ts) = table_stack.last_mut() {
                            ts.in_row = true;
                            ts.current_row_cells.clear();
                        }
                    }
                    b"w:tc" if !table_stack.is_empty() => {
                        if let Some(ts) = table_stack.last_mut() {
                            ts.in_cell = true;
                            ts.current_cell_paragraphs.clear();
                            ts.current_cell_props = RawTableCellProperties::default();
                        }
                    }
                    b"w:tcPr" if !table_stack.is_empty() => {
                        if let Some(ts) = table_stack.last_mut() {
                            ts.in_cell_props = true;
                        }
                    }
                    b"w:sectPr" if in_body && table_stack.is_empty() => {
                        let props = parse_section_properties(reader.inner(), &mut buf)?;
                        body.items.push(RawBodyItem::SectionBreak(props));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:p" if in_body => {
                        // Self-closing paragraph
                        if let Some(ts) = table_stack.last_mut() {
                            if ts.in_cell {
                                ts.current_cell_paragraphs.push(RawParagraph::default());
                            }
                        } else {
                            body.items
                                .push(RawBodyItem::Paragraph(RawParagraph::default()));
                        }
                    }
                    b"w:gridSpan" => {
                        if let Some(ts) = table_stack.last_mut() {
                            if ts.in_cell_props {
                                if let Some(val) = read_w_val(e) {
                                    ts.current_cell_props.grid_span = val.parse().unwrap_or(1);
                                }
                            }
                        }
                    }
                    b"w:vMerge" => {
                        if let Some(ts) = table_stack.last_mut() {
                            if ts.in_cell_props {
                                let val = read_w_val(e);
                                ts.current_cell_props.v_merge = match val.as_deref() {
                                    Some("restart") => Some(RawVMerge::Restart),
                                    _ => Some(RawVMerge::Continue),
                                };
                            }
                        }
                    }
                    b"w:tcW" => {
                        if let Some(ts) = table_stack.last_mut() {
                            if ts.in_cell_props {
                                if let Some(val) = read_attr(e, b"w:w") {
                                    ts.current_cell_props.width = val.parse().ok();
                                }
                            }
                        }
                    }
                    b"w:footnoteReference" if in_paragraph => {
                        if let Some(val) = read_attr(e, b"w:id") {
                            if let Ok(id) = val.parse::<u32>() {
                                current_paragraph.footnote_ref_ids.push(id);
                            }
                        }
                    }
                    b"w:endnoteReference" if in_paragraph => {
                        if let Some(val) = read_attr(e, b"w:id") {
                            if let Ok(id) = val.parse::<u32>() {
                                current_paragraph.endnote_ref_ids.push(id);
                            }
                        }
                    }
                    b"w:commentReference" if in_paragraph => {
                        if let Some(val) = read_attr(e, b"w:id") {
                            if let Ok(id) = val.parse::<u32>() {
                                current_paragraph.comment_ref_ids.push(id);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref t)) if in_text => {
                let text = String::from_utf8_lossy(t).to_string();
                match &mut current_run_text {
                    Some(existing) => existing.push_str(&text),
                    None => current_run_text = Some(text),
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:body" => {
                        in_body = false;
                    }
                    b"w:t" => {
                        in_text = false;
                    }
                    b"w:r" if in_run => {
                        let run = RawRun {
                            text: current_run_text.take(),
                            properties: std::mem::take(&mut current_run_properties),
                        };
                        if in_hyperlink {
                            hyperlink_runs.push(run);
                        } else {
                            current_paragraph.content.push(RawInline::Run(run));
                        }
                        in_run = false;
                    }
                    b"w:hyperlink" if in_hyperlink => {
                        current_paragraph
                            .content
                            .push(RawInline::Hyperlink(RawHyperlink {
                                rel_id: current_hyperlink_rel_id.take(),
                                anchor: current_hyperlink_anchor.take(),
                                runs: std::mem::take(&mut hyperlink_runs),
                            }));
                        in_hyperlink = false;
                    }
                    b"w:p" if in_paragraph => {
                        let para = std::mem::take(&mut current_paragraph);
                        if let Some(ts) = table_stack.last_mut() {
                            if ts.in_cell {
                                ts.current_cell_paragraphs.push(para);
                            }
                        } else {
                            body.items.push(RawBodyItem::Paragraph(para));
                        }
                        in_paragraph = false;
                    }
                    b"w:tcPr" => {
                        if let Some(ts) = table_stack.last_mut() {
                            ts.in_cell_props = false;
                        }
                    }
                    b"w:tc" => {
                        if let Some(ts) = table_stack.last_mut() {
                            ts.finish_cell();
                        }
                    }
                    b"w:tr" => {
                        if let Some(ts) = table_stack.last_mut() {
                            ts.finish_row();
                        }
                    }
                    b"w:tbl" => {
                        if let Some(ts) = table_stack.pop() {
                            let table = ts.into_table();
                            // If there's a parent table (nested), push as body item too
                            // For now, push to body
                            body.items.push(RawBodyItem::Table(table));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: "word/document.xml".into(),
                    reason: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::tables::RawVMerge;

    fn wrap_body(inner: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>
    {inner}
  </w:body>
</w:document>"#
        )
        .into_bytes()
    }

    /// Extracts every `RawInline::Run` from a paragraph's content, in
    /// document order, ignoring hyperlinks. Mirrors the pre-IO-Cycle-1
    /// `paragraph.runs` view for tests that don't care about link order.
    fn runs_of(p: &RawParagraph) -> Vec<&RawRun> {
        p.content
            .iter()
            .filter_map(|i| match i {
                RawInline::Run(r) => Some(r),
                RawInline::Hyperlink(_) => None,
            })
            .collect()
    }

    fn hyperlinks_of(p: &RawParagraph) -> Vec<&RawHyperlink> {
        p.content
            .iter()
            .filter_map(|i| match i {
                RawInline::Hyperlink(h) => Some(h),
                RawInline::Run(_) => None,
            })
            .collect()
    }

    #[test]
    fn parse_single_empty_paragraph() {
        let xml = wrap_body("<w:p/>");
        let body = parse_document_xml(&xml).unwrap();
        assert_eq!(body.items.len(), 1);
        assert!(matches!(body.items[0], RawBodyItem::Paragraph(_)));
    }

    #[test]
    fn parse_paragraph_with_style() {
        let xml = wrap_body(
            r#"<w:p>
              <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Paragraph(ref p) = body.items[0] {
            assert_eq!(p.properties.style_id.as_deref(), Some("Heading1"));
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn parse_paragraph_with_alignment() {
        let xml = wrap_body(
            r#"<w:p>
              <w:pPr><w:jc w:val="center"/></w:pPr>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Paragraph(ref p) = body.items[0] {
            assert_eq!(p.properties.alignment.as_deref(), Some("center"));
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn parse_num_pr() {
        let xml = wrap_body(
            r#"<w:p>
              <w:pPr>
                <w:numPr>
                  <w:ilvl w:val="0"/>
                  <w:numId w:val="1"/>
                </w:numPr>
              </w:pPr>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Paragraph(ref p) = body.items[0] {
            let np = p.properties.num_pr.as_ref().unwrap();
            assert_eq!(np.num_id, 1);
            assert_eq!(np.ilvl, 0);
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn parse_run_with_bold_text() {
        let xml = wrap_body(
            r#"<w:p>
              <w:r>
                <w:rPr><w:b/></w:rPr>
                <w:t>Hello</w:t>
              </w:r>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Paragraph(ref p) = body.items[0] {
            let runs = runs_of(p);
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].properties.bold, Some(true));
            assert_eq!(runs[0].text.as_deref(), Some("Hello"));
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn parse_run_with_explicit_false_toggle_yields_some_false() {
        // OOXML CT_OnOff: <w:b w:val="0"/> and <w:i w:val="false"/> are
        // explicit OFF, not absence. The parser must surface Some(false) so
        // the style resolver can override an inherited true. <w:u/> with no
        // w:val stays Some(true) (empty toggle = ON).
        let xml = wrap_body(
            r#"<w:p>
              <w:r>
                <w:rPr>
                  <w:b w:val="0"/>
                  <w:i w:val="false"/>
                  <w:u/>
                </w:rPr>
                <w:t>x</w:t>
              </w:r>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Paragraph(ref p) = body.items[0] {
            let runs = runs_of(p);
            assert_eq!(runs.len(), 1);
            assert_eq!(
                runs[0].properties.bold,
                Some(false),
                "<w:b w:val=\"0\"/> must parse as explicit OFF"
            );
            assert_eq!(
                runs[0].properties.italic,
                Some(false),
                "<w:i w:val=\"false\"/> must parse as explicit OFF"
            );
            assert_eq!(
                runs[0].properties.underline,
                Some(true),
                "<w:u/> with no w:val stays explicit ON"
            );
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn parse_run_color_and_size() {
        let xml = wrap_body(
            r#"<w:p>
              <w:r>
                <w:rPr>
                  <w:sz w:val="24"/>
                  <w:color w:val="FF0000"/>
                </w:rPr>
                <w:t>Red</w:t>
              </w:r>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Paragraph(ref p) = body.items[0] {
            let runs = runs_of(p);
            assert_eq!(runs[0].properties.font_size_half_points, Some(24));
            assert_eq!(runs[0].properties.color.as_deref(), Some("FF0000"));
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn parse_paragraph_preserves_run_hyperlink_run_order_in_content() {
        // Before IO-Cycle 1, runs and hyperlinks lived in two separate
        // vectors and lost their interleave. Now `content` is a single
        // vector in document order — a run before the link and a run
        // after must surround the hyperlink in the right positions.
        let xml = wrap_body(
            r#"<w:p>
              <w:r><w:t>before </w:t></w:r>
              <w:hyperlink r:id="rId1"><w:r><w:t>link</w:t></w:r></w:hyperlink>
              <w:r><w:t> after</w:t></w:r>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        let RawBodyItem::Paragraph(ref p) = body.items[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(p.content.len(), 3);
        match &p.content[0] {
            RawInline::Run(r) => assert_eq!(r.text.as_deref(), Some("before ")),
            other => panic!("expected Run, got {other:?}"),
        }
        match &p.content[1] {
            RawInline::Hyperlink(h) => {
                assert_eq!(h.rel_id.as_deref(), Some("rId1"));
                assert_eq!(h.runs.len(), 1);
                assert_eq!(h.runs[0].text.as_deref(), Some("link"));
            }
            other => panic!("expected Hyperlink, got {other:?}"),
        }
        match &p.content[2] {
            RawInline::Run(r) => assert_eq!(r.text.as_deref(), Some(" after")),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn parse_hyperlink() {
        let xml = wrap_body(
            r#"<w:p>
              <w:hyperlink r:id="rId5">
                <w:r><w:t>Click</w:t></w:r>
              </w:hyperlink>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Paragraph(ref p) = body.items[0] {
            let links = hyperlinks_of(p);
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].rel_id.as_deref(), Some("rId5"));
            assert_eq!(links[0].runs.len(), 1);
            assert_eq!(links[0].runs[0].text.as_deref(), Some("Click"));
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn parse_minimal_table() {
        let xml = wrap_body(
            r#"<w:tbl>
              <w:tr>
                <w:tc>
                  <w:p><w:r><w:t>Cell</w:t></w:r></w:p>
                </w:tc>
              </w:tr>
            </w:tbl>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        assert_eq!(body.items.len(), 1);
        if let RawBodyItem::Table(ref table) = body.items[0] {
            assert_eq!(table.rows.len(), 1);
            assert_eq!(table.rows[0].cells.len(), 1);
            assert_eq!(table.rows[0].cells[0].paragraphs.len(), 1);
            let text = runs_of(&table.rows[0].cells[0].paragraphs[0])[0]
                .text
                .as_deref();
            assert_eq!(text, Some("Cell"));
        } else {
            panic!("Expected table");
        }
    }

    #[test]
    fn parse_table_2x2() {
        let xml = wrap_body(
            r#"<w:tbl>
              <w:tr>
                <w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc>
              </w:tr>
              <w:tr>
                <w:tc><w:p><w:r><w:t>A2</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>B2</w:t></w:r></w:p></w:tc>
              </w:tr>
            </w:tbl>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Table(ref table) = body.items[0] {
            assert_eq!(table.rows.len(), 2);
            assert_eq!(table.rows[0].cells.len(), 2);
            assert_eq!(table.rows[1].cells.len(), 2);
        } else {
            panic!("Expected table");
        }
    }

    #[test]
    fn parse_cell_grid_span() {
        let xml = wrap_body(
            r#"<w:tbl>
              <w:tr>
                <w:tc>
                  <w:tcPr><w:gridSpan w:val="2"/></w:tcPr>
                  <w:p/>
                </w:tc>
              </w:tr>
            </w:tbl>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Table(ref table) = body.items[0] {
            assert_eq!(table.rows[0].cells[0].properties.grid_span, 2);
        } else {
            panic!("Expected table");
        }
    }

    #[test]
    fn parse_cell_v_merge_restart() {
        let xml = wrap_body(
            r#"<w:tbl>
              <w:tr>
                <w:tc>
                  <w:tcPr><w:vMerge w:val="restart"/></w:tcPr>
                  <w:p/>
                </w:tc>
              </w:tr>
            </w:tbl>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Table(ref table) = body.items[0] {
            assert_eq!(
                table.rows[0].cells[0].properties.v_merge,
                Some(RawVMerge::Restart)
            );
        } else {
            panic!("Expected table");
        }
    }

    #[test]
    fn parse_cell_v_merge_continue() {
        let xml = wrap_body(
            r#"<w:tbl>
              <w:tr>
                <w:tc>
                  <w:tcPr><w:vMerge/></w:tcPr>
                  <w:p/>
                </w:tc>
              </w:tr>
            </w:tbl>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Table(ref table) = body.items[0] {
            assert_eq!(
                table.rows[0].cells[0].properties.v_merge,
                Some(RawVMerge::Continue)
            );
        } else {
            panic!("Expected table");
        }
    }

    #[test]
    fn parse_section_break() {
        let xml = wrap_body(
            r#"<w:p/>
            <w:sectPr>
              <w:pgSz w:w="12240" w:h="15840"/>
            </w:sectPr>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        assert_eq!(body.items.len(), 2);
        assert!(matches!(body.items[1], RawBodyItem::SectionBreak(_)));
    }

    #[test]
    fn parse_header_xml_extracts_paragraphs_from_w_hdr_envelope() {
        let xml = br#"<?xml version="1.0"?>
<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:p><w:r><w:t>Page header</w:t></w:r></w:p>
</w:hdr>"#;
        let body = parse_header_xml(xml).expect("parse_header_xml");
        assert_eq!(body.items.len(), 1);
        let RawBodyItem::Paragraph(p) = &body.items[0] else {
            panic!("expected paragraph, got {:?}", body.items[0]);
        };
        let runs = runs_of(p);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text.as_deref(), Some("Page header"));
    }

    #[test]
    fn parse_footer_xml_extracts_paragraphs_from_w_ftr_envelope() {
        let xml = br#"<?xml version="1.0"?>
<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:p><w:r><w:t>Page 1 of 10</w:t></w:r></w:p>
</w:ftr>"#;
        let body = parse_footer_xml(xml).expect("parse_footer_xml");
        assert_eq!(body.items.len(), 1);
        let RawBodyItem::Paragraph(p) = &body.items[0] else {
            panic!("expected paragraph, got {:?}", body.items[0]);
        };
        assert_eq!(runs_of(p)[0].text.as_deref(), Some("Page 1 of 10"));
    }

    #[test]
    fn parse_section_break_captures_header_and_footer_references() {
        use crate::raw::body::SectionRefType;

        let xml = wrap_body(
            r#"<w:p/>
            <w:sectPr>
              <w:headerReference w:type="default" r:id="rId10"/>
              <w:headerReference w:type="first" r:id="rId11"/>
              <w:footerReference w:type="even" r:id="rId12"/>
              <w:pgSz w:w="12240" w:h="15840"/>
            </w:sectPr>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        // section break + paragraph
        assert_eq!(body.items.len(), 2);
        let RawBodyItem::SectionBreak(props) = &body.items[1] else {
            panic!(
                "expected SectionBreak with properties, got {:?}",
                body.items[1]
            );
        };
        assert_eq!(props.header_refs.len(), 2);
        assert_eq!(props.header_refs[0].rel_id, "rId10");
        assert_eq!(props.header_refs[0].ref_type, SectionRefType::Default);
        assert_eq!(props.header_refs[1].rel_id, "rId11");
        assert_eq!(props.header_refs[1].ref_type, SectionRefType::First);
        assert_eq!(props.footer_refs.len(), 1);
        assert_eq!(props.footer_refs[0].rel_id, "rId12");
        assert_eq!(props.footer_refs[0].ref_type, SectionRefType::Even);
    }

    #[test]
    fn parse_multiple_paragraphs() {
        let xml = wrap_body(
            r#"<w:p><w:r><w:t>First</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second</w:t></w:r></w:p>
            <w:p><w:r><w:t>Third</w:t></w:r></w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        assert_eq!(body.items.len(), 3);
        for item in &body.items {
            assert!(matches!(item, RawBodyItem::Paragraph(_)));
        }
    }

    #[test]
    fn parse_multiple_runs_in_paragraph() {
        let xml = wrap_body(
            r#"<w:p>
              <w:r><w:rPr><w:b/></w:rPr><w:t>Bold </w:t></w:r>
              <w:r><w:rPr><w:i/></w:rPr><w:t>Italic</w:t></w:r>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        if let RawBodyItem::Paragraph(ref p) = body.items[0] {
            let runs = runs_of(p);
            assert_eq!(runs.len(), 2);
            assert_eq!(runs[0].properties.bold, Some(true));
            assert_eq!(runs[0].text.as_deref(), Some("Bold "));
            assert_eq!(runs[1].properties.italic, Some(true));
            assert_eq!(runs[1].text.as_deref(), Some("Italic"));
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn paragraph_captures_comment_reference_ids_from_runs() {
        let xml = wrap_body(
            r#"<w:p>
                <w:r><w:commentReference w:id="0"/></w:r>
                <w:r><w:t>text</w:t></w:r>
                <w:r><w:commentReference w:id="3"/></w:r>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        let RawBodyItem::Paragraph(p) = &body.items[0] else {
            panic!("expected Paragraph");
        };
        assert_eq!(p.comment_ref_ids, vec![0, 3]);
        assert!(p.footnote_ref_ids.is_empty());
        assert!(p.endnote_ref_ids.is_empty());
    }

    #[test]
    fn paragraph_captures_endnote_reference_ids_from_runs() {
        let xml = wrap_body(
            r#"<w:p>
                <w:r><w:t>cite</w:t></w:r>
                <w:r><w:endnoteReference w:id="2"/></w:r>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        let RawBodyItem::Paragraph(p) = &body.items[0] else {
            panic!("expected Paragraph");
        };
        assert_eq!(p.endnote_ref_ids, vec![2]);
        assert!(
            p.footnote_ref_ids.is_empty(),
            "endnote ref must not leak into footnote bucket"
        );
    }

    #[test]
    fn paragraph_captures_footnote_reference_ids_from_runs() {
        let xml = wrap_body(
            r#"<w:p>
                <w:r><w:t>See</w:t></w:r>
                <w:r><w:footnoteReference w:id="1"/></w:r>
                <w:r><w:t> and</w:t></w:r>
                <w:r><w:footnoteReference w:id="3"/></w:r>
            </w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        assert_eq!(body.items.len(), 1);
        let RawBodyItem::Paragraph(p) = &body.items[0] else {
            panic!("expected Paragraph");
        };
        assert_eq!(p.footnote_ref_ids, vec![1, 3]);
    }

    #[test]
    fn parse_paragraph_then_table_then_paragraph() {
        let xml = wrap_body(
            r#"<w:p><w:r><w:t>Before</w:t></w:r></w:p>
            <w:tbl>
              <w:tr><w:tc><w:p><w:r><w:t>Cell</w:t></w:r></w:p></w:tc></w:tr>
            </w:tbl>
            <w:p><w:r><w:t>After</w:t></w:r></w:p>"#,
        );
        let body = parse_document_xml(&xml).unwrap();
        assert_eq!(body.items.len(), 3);
        assert!(matches!(body.items[0], RawBodyItem::Paragraph(_)));
        assert!(matches!(body.items[1], RawBodyItem::Table(_)));
        assert!(matches!(body.items[2], RawBodyItem::Paragraph(_)));
    }
}
