use quick_xml::events::Event;

use crate::error::{DocxError, Result};
use crate::numbering::defs::{
    AbstractNum, ConcreteNum, NumberingDefs, NumberingLevel, NumberingLevelOverride,
};
use crate::raw::paragraphs::RawParagraphProperties;
use crate::raw::runs::RawRunProperties;
use crate::word::styles_xml::{parse_paragraph_properties, parse_run_properties};
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

/// Parses `word/numbering.xml` into `NumberingDefs`.
pub(crate) fn parse_numbering_xml(xml_bytes: &[u8]) -> Result<NumberingDefs> {
    let mut reader = XmlReader::from_bytes(xml_bytes, "word/numbering.xml")?;
    let mut defs = NumberingDefs::new();
    let mut buf = Vec::new();

    // State for abstractNum
    let mut in_abstract_num = false;
    let mut current_abstract_num_id: Option<u32> = None;
    let mut current_levels: Vec<NumberingLevel> = Vec::new();

    // State for lvl
    let mut in_lvl = false;
    let mut current_ilvl: Option<u8> = None;
    let mut current_start: u32 = 1;
    let mut current_num_fmt = String::new();
    let mut current_level_text = String::new();
    let mut current_indent_left: Option<u32> = None;
    let mut current_indent_hanging: Option<u32> = None;
    let mut current_lvl_rpr: Option<RawRunProperties> = None;
    let mut current_lvl_ppr: Option<RawParagraphProperties> = None;

    // State for num (concrete)
    let mut in_num = false;
    let mut current_num_id: Option<u32> = None;
    let mut current_concrete_abstract_id: Option<u32> = None;
    let mut current_level_overrides: Vec<NumberingLevelOverride> = Vec::new();

    // State for lvlOverride
    let mut in_lvl_override = false;
    let mut current_override_ilvl: Option<u8> = None;
    let mut current_override_start: Option<u32> = None;

    loop {
        match reader.inner().read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:abstractNum" => {
                        in_abstract_num = true;
                        current_levels.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:abstractNumId" {
                                let val = String::from_utf8_lossy(&attr.value);
                                current_abstract_num_id = val.parse().ok();
                            }
                        }
                    }
                    b"w:lvl" if in_abstract_num => {
                        in_lvl = true;
                        current_start = 1;
                        current_num_fmt.clear();
                        current_level_text.clear();
                        current_indent_left = None;
                        current_indent_hanging = None;
                        current_lvl_rpr = None;
                        current_lvl_ppr = None;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:ilvl" {
                                let val = String::from_utf8_lossy(&attr.value);
                                current_ilvl = val.parse().ok();
                            }
                        }
                    }
                    b"w:rPr" if in_lvl => {
                        current_lvl_rpr = Some(parse_run_properties(reader.inner(), &mut buf)?);
                    }
                    b"w:pPr" if in_lvl => {
                        current_lvl_ppr =
                            Some(parse_paragraph_properties(reader.inner(), &mut buf)?);
                    }
                    b"w:num" => {
                        in_num = true;
                        current_concrete_abstract_id = None;
                        current_level_overrides.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:numId" {
                                let val = String::from_utf8_lossy(&attr.value);
                                current_num_id = val.parse().ok();
                            }
                        }
                    }
                    b"w:lvlOverride" if in_num => {
                        in_lvl_override = true;
                        current_override_start = None;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:ilvl" {
                                let val = String::from_utf8_lossy(&attr.value);
                                current_override_ilvl = val.parse().ok();
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:start" if in_lvl => {
                        if let Some(val) = read_w_val(e) {
                            current_start = val.parse().unwrap_or(1);
                        }
                    }
                    b"w:numFmt" if in_lvl => {
                        if let Some(val) = read_w_val(e) {
                            current_num_fmt = val;
                        }
                    }
                    b"w:lvlText" if in_lvl => {
                        if let Some(val) = read_w_val(e) {
                            current_level_text = val;
                        }
                    }
                    b"w:abstractNumId" if in_num => {
                        if let Some(val) = read_w_val(e) {
                            current_concrete_abstract_id = val.parse().ok();
                        }
                    }
                    b"w:startOverride" if in_lvl_override => {
                        if let Some(val) = read_w_val(e) {
                            current_override_start = val.parse().ok();
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                match local {
                    b"w:lvl" if in_lvl => {
                        if let Some(ilvl) = current_ilvl.take() {
                            current_levels.push(NumberingLevel {
                                ilvl,
                                start: current_start,
                                num_fmt: std::mem::take(&mut current_num_fmt),
                                level_text: std::mem::take(&mut current_level_text),
                                indent_left: current_indent_left.take(),
                                indent_hanging: current_indent_hanging.take(),
                                run_properties: current_lvl_rpr.take(),
                                paragraph_properties: current_lvl_ppr.take(),
                            });
                        }
                        in_lvl = false;
                    }
                    b"w:abstractNum" if in_abstract_num => {
                        if let Some(id) = current_abstract_num_id.take() {
                            defs.insert_abstract(AbstractNum {
                                abstract_num_id: id,
                                levels: std::mem::take(&mut current_levels),
                            });
                        }
                        in_abstract_num = false;
                    }
                    b"w:lvlOverride" if in_lvl_override => {
                        if let Some(ilvl) = current_override_ilvl.take() {
                            current_level_overrides.push(NumberingLevelOverride {
                                ilvl,
                                start_override: current_override_start.take(),
                            });
                        }
                        in_lvl_override = false;
                    }
                    b"w:num" if in_num => {
                        if let (Some(num_id), Some(abstract_num_id)) =
                            (current_num_id.take(), current_concrete_abstract_id.take())
                        {
                            defs.insert_concrete(ConcreteNum {
                                num_id,
                                abstract_num_id,
                                level_overrides: std::mem::take(&mut current_level_overrides),
                            });
                        }
                        in_num = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: "word/numbering.xml".into(),
                    reason: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(defs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_numbering() {
        let xml = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
</w:numbering>"#;
        let defs = parse_numbering_xml(xml).unwrap();
        assert!(defs.resolve(1, 0).is_err());
    }

    #[test]
    fn parse_single_abstract_and_concrete() {
        let xml = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="decimal"/>
      <w:lvlText w:val="%1."/>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1">
    <w:abstractNumId w:val="0"/>
  </w:num>
</w:numbering>"#;
        let defs = parse_numbering_xml(xml).unwrap();
        let level = defs.resolve(1, 0).unwrap();
        assert_eq!(level.num_fmt, "decimal");
        assert_eq!(level.start, 1);
        assert_eq!(level.level_text, "%1.");
        assert_eq!(level.ilvl, 0);
    }

    #[test]
    fn parse_multiple_levels() {
        let xml = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="decimal"/>
      <w:lvlText w:val="%1."/>
    </w:lvl>
    <w:lvl w:ilvl="1">
      <w:start w:val="1"/>
      <w:numFmt w:val="lowerLetter"/>
      <w:lvlText w:val="%2)"/>
    </w:lvl>
    <w:lvl w:ilvl="2">
      <w:start w:val="1"/>
      <w:numFmt w:val="lowerRoman"/>
      <w:lvlText w:val="%3."/>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1">
    <w:abstractNumId w:val="0"/>
  </w:num>
</w:numbering>"#;
        let defs = parse_numbering_xml(xml).unwrap();
        let l0 = defs.resolve(1, 0).unwrap();
        assert_eq!(l0.num_fmt, "decimal");
        let l1 = defs.resolve(1, 1).unwrap();
        assert_eq!(l1.num_fmt, "lowerLetter");
        let l2 = defs.resolve(1, 2).unwrap();
        assert_eq!(l2.num_fmt, "lowerRoman");
        assert_eq!(l2.ilvl, 2);
    }

    #[test]
    fn parse_bullet_list() {
        let xml = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="1">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="bullet"/>
      <w:lvlText w:val="&#61623;"/>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="2">
    <w:abstractNumId w:val="1"/>
  </w:num>
</w:numbering>"#;
        let defs = parse_numbering_xml(xml).unwrap();
        let level = defs.resolve(2, 0).unwrap();
        assert_eq!(level.num_fmt, "bullet");
    }

    #[test]
    fn parse_level_captures_rpr_bold_and_ppr_outline_level() {
        // List levels carry their own <w:rPr> and <w:pPr> in OOXML.
        // Those are the "list-level" layer of the 4-layer style chain
        // (docDefaults → basedOn chain → list-level → direct). The parser
        // must surface them so StyleResolver can merge them in.
        let xml = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="decimal"/>
      <w:lvlText w:val="%1."/>
      <w:pPr>
        <w:outlineLvl w:val="0"/>
      </w:pPr>
      <w:rPr>
        <w:b/>
      </w:rPr>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1">
    <w:abstractNumId w:val="0"/>
  </w:num>
</w:numbering>"#;
        let defs = parse_numbering_xml(xml).unwrap();
        let level = defs.resolve(1, 0).unwrap();

        let rpr = level
            .run_properties
            .as_ref()
            .expect("level rPr should be captured");
        assert_eq!(
            rpr.bold,
            Some(true),
            "<w:b/> in level rPr must surface as bold=Some(true)"
        );

        let ppr = level
            .paragraph_properties
            .as_ref()
            .expect("level pPr should be captured");
        assert_eq!(
            ppr.outline_level,
            Some(0),
            "<w:outlineLvl w:val=\"0\"/> in level pPr must surface"
        );
    }

    #[test]
    fn missing_num_id_returns_error() {
        let xml = br#"<?xml version="1.0"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="decimal"/>
      <w:lvlText w:val="%1."/>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1">
    <w:abstractNumId w:val="0"/>
  </w:num>
</w:numbering>"#;
        let defs = parse_numbering_xml(xml).unwrap();
        assert!(defs.resolve(99, 0).is_err());
    }
}
