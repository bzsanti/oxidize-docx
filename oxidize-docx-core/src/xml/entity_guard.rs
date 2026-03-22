use crate::error::{DocxError, Result};

#[allow(dead_code)]
pub const MAX_ENTITY_EXPANSIONS: usize = 100;
#[allow(dead_code)]
pub const MAX_XML_DEPTH: usize = 256;

/// Scans the first bytes of XML content for a DOCTYPE declaration.
///
/// Only scans the first 4096 bytes — DOCTYPE must appear before the root element.
pub fn contains_doctype(xml_bytes: &[u8]) -> bool {
    let scan_limit = xml_bytes.len().min(4096);
    let scan = &xml_bytes[..scan_limit];

    // Case-insensitive search for <!DOCTYPE
    for window in scan.windows(9) {
        if window.eq_ignore_ascii_case(b"<!DOCTYPE") {
            return true;
        }
    }
    false
}

/// Returns an error if the XML bytes contain a DOCTYPE declaration.
///
/// OOXML documents must never contain DOCTYPE declarations — their presence
/// indicates a potential XML entity expansion attack (billion laughs).
pub fn reject_if_doctype(xml_bytes: &[u8], part_name: &str) -> Result<()> {
    if contains_doctype(xml_bytes) {
        return Err(DocxError::XmlParse {
            part: part_name.to_string(),
            reason: "DOCTYPE declaration detected — rejected for security".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_constants_are_correct() {
        assert_eq!(MAX_ENTITY_EXPANSIONS, 100);
        assert_eq!(MAX_XML_DEPTH, 256);
    }

    #[test]
    fn doctype_detection_finds_doctype() {
        let xml = b"<?xml version=\"1.0\"?><!DOCTYPE foo [...]><root/>";
        assert!(contains_doctype(xml));
    }

    #[test]
    fn doctype_detection_finds_case_insensitive() {
        let xml = b"<?xml version=\"1.0\"?><!doctype foo><root/>";
        assert!(contains_doctype(xml));
    }

    #[test]
    fn doctype_detection_allows_clean_xml() {
        let xml = b"<?xml version=\"1.0\"?><root><child/></root>";
        assert!(!contains_doctype(xml));
    }

    #[test]
    fn reject_xml_with_doctype_returns_error() {
        let xml = b"<!DOCTYPE bomb [<!ENTITY a \"b\">]><root/>";
        let result = reject_if_doctype(xml, "word/document.xml");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DocxError::XmlParse { .. }));
    }

    #[test]
    fn reject_xml_without_doctype_passes() {
        let clean = b"<root><w:p xmlns:w=\"http://example.com\"/></root>";
        assert!(reject_if_doctype(clean, "word/document.xml").is_ok());
    }

    #[test]
    fn doctype_in_billion_laughs_attack() {
        let attack = br#"<?xml version="1.0"?>
<!DOCTYPE lolz [
  <!ENTITY lol "lol">
  <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
]>
<root>&lol2;</root>"#;
        assert!(contains_doctype(attack));
        assert!(reject_if_doctype(attack, "evil.xml").is_err());
    }
}
