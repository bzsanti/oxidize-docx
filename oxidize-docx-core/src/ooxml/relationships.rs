use std::collections::HashMap;

use quick_xml::events::Event;

use crate::error::{DocxError, Result};
use crate::xml::reader::XmlReader;

/// A single OOXML relationship entry.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct Relationship {
    pub(crate) id: String,
    pub(crate) rel_type: String,
    pub(crate) target: String,
    pub(crate) is_external: bool,
}

/// Parsed representation of an OOXML `.rels` file.
#[allow(dead_code)]
pub(crate) struct RelationshipMap {
    rels: HashMap<String, Relationship>, // id -> Relationship
}

#[allow(dead_code)]
impl RelationshipMap {
    /// Parses a `.rels` file from raw XML bytes.
    ///
    /// `source_path` is used for error messages (e.g., `"_rels/.rels"`).
    pub(crate) fn parse(xml_bytes: &[u8], source_path: &str) -> Result<Self> {
        let mut reader = XmlReader::from_bytes(xml_bytes, source_path)?;
        let mut rels = HashMap::new();
        let mut buf = Vec::new();

        loop {
            match reader.inner().read_event_into(&mut buf) {
                Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                    let name = e.name();
                    let local = name.as_ref();
                    if local == b"Relationship" {
                        let mut id = None;
                        let mut rel_type = None;
                        let mut target = None;
                        let mut is_external = false;

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"Id" => {
                                    id = Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                                b"Type" => {
                                    rel_type =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                                b"Target" => {
                                    target = Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                                b"TargetMode" => {
                                    let val = String::from_utf8_lossy(&attr.value).to_string();
                                    is_external = val.eq_ignore_ascii_case("External");
                                }
                                _ => {}
                            }
                        }

                        if let (Some(id), Some(rel_type), Some(target)) = (id, rel_type, target) {
                            rels.insert(
                                id.clone(),
                                Relationship {
                                    id,
                                    rel_type,
                                    target,
                                    is_external,
                                },
                            );
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(DocxError::InvalidRelationships {
                        path: source_path.to_string(),
                        reason: e.to_string(),
                    });
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(Self { rels })
    }

    /// Gets a relationship by its ID (e.g., "rId1").
    pub(crate) fn get_by_id(&self, id: &str) -> Option<&Relationship> {
        self.rels.get(id)
    }

    /// Finds all relationships whose type contains the given suffix.
    pub(crate) fn find_by_type(&self, type_suffix: &str) -> Vec<&Relationship> {
        self.rels
            .values()
            .filter(|r| r.rel_type.contains(type_suffix))
            .collect()
    }

    /// Returns the total number of relationships.
    pub(crate) fn len(&self) -> usize {
        self.rels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>
  <Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/>
</Relationships>"#;

    #[test]
    fn parse_relationships_file() {
        let rels = RelationshipMap::parse(DOCUMENT_RELS, "_rels/document.xml.rels").unwrap();
        assert_eq!(rels.len(), 4);
    }

    #[test]
    fn find_relationship_by_id() {
        let rels = RelationshipMap::parse(DOCUMENT_RELS, "_rels/document.xml.rels").unwrap();
        let rel = rels.get_by_id("rId1");
        assert!(rel.is_some());
        assert!(rel.unwrap().target.contains("styles.xml"));
    }

    #[test]
    fn find_relationships_by_type() {
        let rels = RelationshipMap::parse(DOCUMENT_RELS, "_rels/document.xml.rels").unwrap();
        let images = rels.find_by_type("relationships/image");
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn external_relationship_is_flagged() {
        let rels = RelationshipMap::parse(DOCUMENT_RELS, "_rels/document.xml.rels").unwrap();
        let hyperlink = rels.get_by_id("rId4").unwrap();
        assert!(hyperlink.is_external);
    }

    #[test]
    fn internal_relationship_is_not_external() {
        let rels = RelationshipMap::parse(DOCUMENT_RELS, "_rels/document.xml.rels").unwrap();
        let styles = rels.get_by_id("rId1").unwrap();
        assert!(!styles.is_external);
    }

    #[test]
    fn missing_id_returns_none() {
        let rels = RelationshipMap::parse(DOCUMENT_RELS, "_rels/document.xml.rels").unwrap();
        assert!(rels.get_by_id("rId99").is_none());
    }

    #[test]
    fn reject_malformed_rels() {
        // Mismatched tags trigger quick-xml error
        let bad = b"<Relationships></Wrong>";
        let result = RelationshipMap::parse(bad, "_rels/.rels");
        assert!(result.is_err());
    }

    #[test]
    fn truncated_xml_parses_to_empty_map() {
        let truncated = b"<Relationships>UNCLOSED";
        let rels = RelationshipMap::parse(truncated, "_rels/.rels").unwrap();
        assert_eq!(rels.len(), 0);
    }
}
