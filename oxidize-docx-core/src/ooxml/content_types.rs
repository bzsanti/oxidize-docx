use std::collections::HashMap;

use quick_xml::events::Event;

use crate::error::{DocxError, Result};
use crate::xml::reader::XmlReader;

const MAIN_DOCUMENT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

/// Parsed representation of `[Content_Types].xml`.
///
/// Maps file extensions to default content types, and specific part names
/// to override content types.
pub(crate) struct ContentTypeMap {
    overrides: HashMap<String, String>, // PartName (without leading /) -> ContentType
    defaults: HashMap<String, String>,  // Extension -> ContentType
}

impl ContentTypeMap {
    /// Parses `[Content_Types].xml` from raw XML bytes.
    pub(crate) fn parse(xml_bytes: &[u8]) -> Result<Self> {
        let mut reader = XmlReader::from_bytes(xml_bytes, "[Content_Types].xml")?;
        let mut overrides = HashMap::new();
        let mut defaults = HashMap::new();
        let mut buf = Vec::new();

        loop {
            match reader.inner().read_event_into(&mut buf) {
                Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    let local = name.as_ref();

                    if local == b"Override" {
                        let mut part_name = None;
                        let mut content_type = None;

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"PartName" => {
                                    let val = String::from_utf8_lossy(&attr.value);
                                    // Strip leading '/' for consistent lookup
                                    part_name = Some(val.trim_start_matches('/').to_string());
                                }
                                b"ContentType" => {
                                    content_type =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                                _ => {}
                            }
                        }

                        if let (Some(pn), Some(ct)) = (part_name, content_type) {
                            overrides.insert(pn, ct);
                        }
                    } else if local == b"Default" {
                        let mut extension = None;
                        let mut content_type = None;

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"Extension" => {
                                    extension =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                                b"ContentType" => {
                                    content_type =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                                _ => {}
                            }
                        }

                        if let (Some(ext), Some(ct)) = (extension, content_type) {
                            defaults.insert(ext, ct);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(DocxError::InvalidContentTypes(e.to_string()));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(Self {
            overrides,
            defaults,
        })
    }

    /// Returns true if the content types manifest contains an override for the given part name.
    ///
    /// `part_name` should NOT have a leading `/`.
    #[allow(dead_code)]
    pub(crate) fn has_part(&self, part_name: &str) -> bool {
        let normalized = part_name.trim_start_matches('/');
        self.overrides.contains_key(normalized)
    }

    /// Finds the first part name that matches the given content type.
    pub(crate) fn find_part_by_content_type(&self, ct: &str) -> Option<String> {
        self.overrides
            .iter()
            .find(|(_, v)| v.as_str() == ct)
            .map(|(k, _)| k.clone())
    }

    /// Returns the path to the main document part (word/document.xml typically).
    pub(crate) fn main_document_part(&self) -> Option<String> {
        self.find_part_by_content_type(MAIN_DOCUMENT_CONTENT_TYPE)
    }

    /// Returns the default content type for a given file extension.
    #[allow(dead_code)]
    pub(crate) fn default_for_extension(&self, ext: &str) -> Option<&str> {
        self.defaults.get(ext).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_CONTENT_TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>
</Types>"#;

    #[test]
    fn parse_minimal_content_types() {
        let ct = ContentTypeMap::parse(MINIMAL_CONTENT_TYPES).unwrap();
        assert!(ct.has_part("/word/document.xml"));
        assert!(ct.has_part("word/document.xml")); // without leading /
        assert!(ct.has_part("/word/styles.xml"));
    }

    #[test]
    fn find_main_document_part() {
        let ct = ContentTypeMap::parse(MINIMAL_CONTENT_TYPES).unwrap();
        let path = ct.main_document_part();
        assert!(path.is_some());
        assert_eq!(path.unwrap(), "word/document.xml");
    }

    #[test]
    fn find_styles_part() {
        let ct = ContentTypeMap::parse(MINIMAL_CONTENT_TYPES).unwrap();
        let path = ct.find_part_by_content_type(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        );
        assert!(path.is_some());
        assert_eq!(path.unwrap(), "word/styles.xml");
    }

    #[test]
    fn numbering_part_found() {
        let ct = ContentTypeMap::parse(MINIMAL_CONTENT_TYPES).unwrap();
        assert!(ct
            .find_part_by_content_type(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"
            )
            .is_some());
    }

    #[test]
    fn default_extension_found() {
        let ct = ContentTypeMap::parse(MINIMAL_CONTENT_TYPES).unwrap();
        let rels_ct = ct.default_for_extension("rels");
        assert!(rels_ct.is_some());
        assert!(rels_ct.unwrap().contains("relationships"));
    }

    #[test]
    fn reject_malformed_xml() {
        // Mismatched tags cause quick-xml to error
        let bad = b"<Types></Wrong>";
        let result = ContentTypeMap::parse(bad);
        assert!(result.is_err());
    }

    #[test]
    fn empty_xml_parses_to_empty_map() {
        // Truncated XML just hits EOF — valid parse, but no entries
        let truncated = b"<Types>NOT CLOSED";
        let ct = ContentTypeMap::parse(truncated).unwrap();
        assert!(ct.main_document_part().is_none());
    }

    #[test]
    fn main_document_part_absent_returns_none() {
        let no_doc = br#"<?xml version="1.0"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
</Types>"#;
        let ct = ContentTypeMap::parse(no_doc).unwrap();
        assert!(ct.main_document_part().is_none());
    }
}
