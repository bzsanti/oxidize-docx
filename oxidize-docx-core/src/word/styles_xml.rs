use quick_xml::events::Event;

use crate::error::{DocxError, Result};
use crate::raw::paragraphs::{RawNumPr, RawParagraphProperties};
use crate::raw::runs::RawRunProperties;
use crate::styles::table::{StyleEntry, StyleTable, StyleType};
use crate::xml::reader::XmlReader;

/// Reads a `w:val` attribute from a quick-xml element.
fn read_w_val(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"w:val" {
            return Some(String::from_utf8_lossy(&attr.value).to_string());
        }
    }
    None
}

/// Resolves an OOXML toggle property (`CT_OnOff`, ECMA-376 §17.17.4) to its
/// boolean value. An absent `w:val` means ON (the element's mere presence
/// asserts the property). `w:val` of "false", "0", or "off" means explicit
/// OFF; any other value means ON.
fn read_toggle_val(e: &quick_xml::events::BytesStart<'_>) -> bool {
    !matches!(
        read_w_val(e).as_deref(),
        Some("false") | Some("0") | Some("off")
    )
}

/// Parses run properties from XML events until `</w:rPr>` is encountered.
///
/// Expects the reader to be positioned just after `<w:rPr>` (Start event consumed).
/// Returns after consuming the matching `</w:rPr>` End event.
pub(crate) fn parse_run_properties(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<RawRunProperties> {
    let mut rpr = RawRunProperties::default();
    let mut depth = 1u32;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local == b"w:rPr" {
                    depth += 1;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:b" => rpr.bold = Some(read_toggle_val(e)),
                    b"w:i" => rpr.italic = Some(read_toggle_val(e)),
                    // w:u is CT_Underline (style enum: single/double/none/…),
                    // not a true CT_OnOff. read_toggle_val maps w:val="none"
                    // to ON, which is wrong for "remove inherited underline".
                    // Pre-existing behavior; a dedicated cycle should map the
                    // enum (none → Some(false), any style → Some(true)).
                    b"w:u" => rpr.underline = Some(read_toggle_val(e)),
                    b"w:strike" => rpr.strikethrough = Some(read_toggle_val(e)),
                    b"w:sz" => {
                        if let Some(val) = read_w_val(e) {
                            rpr.font_size_half_points = val.parse().ok();
                        }
                    }
                    b"w:color" => {
                        rpr.color = read_w_val(e);
                    }
                    b"w:highlight" => {
                        rpr.highlight = read_w_val(e);
                    }
                    b"w:vertAlign" => {
                        rpr.vertical_align = read_w_val(e);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local == b"w:rPr" {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: "rPr".into(),
                    reason: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(rpr)
}

/// Parses paragraph properties from XML events until `</w:pPr>` is encountered.
pub(crate) fn parse_paragraph_properties(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<RawParagraphProperties> {
    let mut ppr = RawParagraphProperties::default();
    let mut depth = 1u32;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:pPr" => {
                        depth += 1;
                    }
                    b"w:numPr" => {
                        // Parse numPr children: <w:ilvl> and <w:numId>
                        let mut ilvl: Option<u8> = None;
                        let mut num_id: Option<u32> = None;
                        loop {
                            match reader.read_event_into(buf) {
                                Ok(Event::Empty(ref inner_e)) => {
                                    let inner_name = inner_e.name();
                                    let inner_local = inner_name.as_ref();
                                    match inner_local {
                                        b"w:ilvl" => {
                                            if let Some(val) = read_w_val(inner_e) {
                                                ilvl = val.parse().ok();
                                            }
                                        }
                                        b"w:numId" => {
                                            if let Some(val) = read_w_val(inner_e) {
                                                num_id = val.parse().ok();
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                Ok(Event::End(ref inner_e)) => {
                                    let inner_name = inner_e.name();
                                    if inner_name.as_ref() == b"w:numPr" {
                                        break;
                                    }
                                }
                                Ok(Event::Eof) => break,
                                Err(_) => break,
                                _ => {}
                            }
                            buf.clear();
                        }
                        if let (Some(num_id), Some(ilvl)) = (num_id, ilvl) {
                            ppr.num_pr = Some(RawNumPr { num_id, ilvl });
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:pStyle" => {
                        ppr.style_id = read_w_val(e);
                    }
                    b"w:jc" => {
                        ppr.alignment = read_w_val(e);
                    }
                    b"w:outlineLvl" => {
                        if let Some(val) = read_w_val(e) {
                            ppr.outline_level = val.parse().ok();
                        }
                    }
                    b"w:keepNext" => ppr.keep_next = true,
                    b"w:pageBreakBefore" => ppr.page_break_before = true,
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local == b"w:pPr" {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: "pPr".into(),
                    reason: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(ppr)
}

fn parse_style_type(val: &str) -> Option<StyleType> {
    match val {
        "paragraph" => Some(StyleType::Paragraph),
        "character" => Some(StyleType::Character),
        "table" => Some(StyleType::Table),
        "numbering" => Some(StyleType::Numbering),
        _ => None,
    }
}

/// Parses `word/styles.xml` into a `StyleTable`.
pub(crate) fn parse_styles_xml(xml_bytes: &[u8]) -> Result<StyleTable> {
    let mut reader = XmlReader::from_bytes(xml_bytes, "word/styles.xml")?;
    let mut table = StyleTable::new();
    let mut buf = Vec::new();

    // State for current style being parsed
    let mut in_style = false;
    let mut current_style_id: Option<String> = None;
    let mut current_style_type: Option<StyleType> = None;
    let mut current_name: Option<String> = None;
    let mut current_based_on: Option<String> = None;
    let mut current_next_style: Option<String> = None;
    let mut current_is_default = false;
    let mut current_rpr: Option<RawRunProperties> = None;
    let mut current_ppr: Option<RawParagraphProperties> = None;

    // State for docDefaults
    let mut in_doc_defaults = false;
    let mut in_rpr_default = false;
    let mut in_ppr_default = false;

    loop {
        match reader.inner().read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:style" => {
                        in_style = true;
                        current_is_default = false;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"w:styleId" => {
                                    current_style_id =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                                b"w:type" => {
                                    let val = String::from_utf8_lossy(&attr.value);
                                    current_style_type = parse_style_type(&val);
                                }
                                b"w:default" => {
                                    let val = String::from_utf8_lossy(&attr.value);
                                    current_is_default = val == "1" || val == "true";
                                }
                                _ => {}
                            }
                        }
                    }
                    b"w:rPr" if in_style => {
                        current_rpr = Some(parse_run_properties(reader.inner(), &mut buf)?);
                    }
                    b"w:pPr" if in_style => {
                        current_ppr = Some(parse_paragraph_properties(reader.inner(), &mut buf)?);
                    }
                    b"w:docDefaults" => {
                        in_doc_defaults = true;
                    }
                    b"w:rPrDefault" if in_doc_defaults => {
                        in_rpr_default = true;
                    }
                    b"w:pPrDefault" if in_doc_defaults => {
                        in_ppr_default = true;
                    }
                    b"w:rPr" if in_rpr_default => {
                        table.doc_defaults_run =
                            Some(parse_run_properties(reader.inner(), &mut buf)?);
                    }
                    b"w:pPr" if in_ppr_default => {
                        table.doc_defaults_paragraph =
                            Some(parse_paragraph_properties(reader.inner(), &mut buf)?);
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if in_style {
                    match local {
                        b"w:name" => {
                            current_name = read_w_val(e);
                        }
                        b"w:basedOn" => {
                            current_based_on = read_w_val(e);
                        }
                        b"w:next" => {
                            current_next_style = read_w_val(e);
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:style" if in_style => {
                        if let (Some(style_id), Some(style_type)) =
                            (current_style_id.take(), current_style_type.take())
                        {
                            table.insert(StyleEntry {
                                style_id,
                                name: current_name.take().unwrap_or_default(),
                                style_type,
                                based_on: current_based_on.take(),
                                next_style: current_next_style.take(),
                                is_default: current_is_default,
                                paragraph_properties: current_ppr.take(),
                                run_properties: current_rpr.take(),
                            });
                        }
                        in_style = false;
                    }
                    b"w:docDefaults" => {
                        in_doc_defaults = false;
                    }
                    b"w:rPrDefault" => {
                        in_rpr_default = false;
                    }
                    b"w:pPrDefault" => {
                        in_ppr_default = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: "word/styles.xml".into(),
                    reason: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_styles_xml() {
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
</w:styles>"#;
        let table = parse_styles_xml(xml).unwrap();
        assert_eq!(table.len(), 0);
        assert!(table.get("Heading1").is_none());
    }

    #[test]
    fn parse_heading1_style() {
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:basedOn w:val="Normal"/>
    <w:next w:val="Normal"/>
  </w:style>
</w:styles>"#;
        let table = parse_styles_xml(xml).unwrap();
        let entry = table.get("Heading1").unwrap();
        assert_eq!(entry.style_id, "Heading1");
        assert_eq!(entry.name, "heading 1");
        assert_eq!(entry.style_type, StyleType::Paragraph);
        assert_eq!(entry.based_on.as_deref(), Some("Normal"));
        assert_eq!(entry.next_style.as_deref(), Some("Normal"));
    }

    #[test]
    fn parse_style_with_run_props() {
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:rPr>
      <w:b/>
      <w:sz w:val="32"/>
      <w:color w:val="2E74B5"/>
    </w:rPr>
  </w:style>
</w:styles>"#;
        let table = parse_styles_xml(xml).unwrap();
        let entry = table.get("Heading1").unwrap();
        let rpr = entry.run_properties.as_ref().unwrap();
        assert_eq!(rpr.bold, Some(true));
        assert_eq!(rpr.font_size_half_points, Some(32));
        assert_eq!(rpr.color.as_deref(), Some("2E74B5"));
    }

    #[test]
    fn parse_style_with_paragraph_props() {
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Centered">
    <w:name w:val="Centered"/>
    <w:pPr>
      <w:jc w:val="center"/>
      <w:keepNext/>
    </w:pPr>
  </w:style>
</w:styles>"#;
        let table = parse_styles_xml(xml).unwrap();
        let entry = table.get("Centered").unwrap();
        let ppr = entry.paragraph_properties.as_ref().unwrap();
        assert_eq!(ppr.alignment.as_deref(), Some("center"));
        assert!(ppr.keep_next);
    }

    #[test]
    fn parse_doc_defaults() {
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault>
      <w:rPr>
        <w:sz w:val="24"/>
      </w:rPr>
    </w:rPrDefault>
  </w:docDefaults>
</w:styles>"#;
        let table = parse_styles_xml(xml).unwrap();
        assert_eq!(
            table
                .doc_defaults_run_properties()
                .and_then(|r| r.font_size_half_points),
            Some(24)
        );
    }

    #[test]
    fn parse_multiple_styles() {
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Normal" w:default="1">
    <w:name w:val="Normal"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:basedOn w:val="Normal"/>
  </w:style>
  <w:style w:type="character" w:styleId="Strong">
    <w:name w:val="Strong"/>
    <w:rPr>
      <w:b/>
    </w:rPr>
  </w:style>
</w:styles>"#;
        let table = parse_styles_xml(xml).unwrap();
        assert_eq!(table.len(), 3);
        assert!(table.get("Normal").unwrap().is_default);
        assert_eq!(
            table.get("Strong").unwrap().style_type,
            StyleType::Character
        );
        assert_eq!(
            table
                .get("Strong")
                .unwrap()
                .run_properties
                .as_ref()
                .unwrap()
                .bold,
            Some(true)
        );
    }

    #[test]
    fn parse_character_style_italic() {
        let xml = br#"<?xml version="1.0"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="character" w:styleId="Emphasis">
    <w:name w:val="Emphasis"/>
    <w:rPr>
      <w:i/>
    </w:rPr>
  </w:style>
</w:styles>"#;
        let table = parse_styles_xml(xml).unwrap();
        let entry = table.get("Emphasis").unwrap();
        assert_eq!(entry.run_properties.as_ref().unwrap().italic, Some(true));
    }
}
