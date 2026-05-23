use std::collections::HashMap;

use crate::error::Result;
use crate::word::notes_common::parse_note_collection;

/// Map of `w:endnote w:id="N"` → concatenated text of the endnote's runs.
/// Mirrors `FootnoteMap` because the OOXML envelope for endnotes is
/// structurally identical to footnotes; only the element names differ.
/// Both share the parser in `notes_common`.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct EndnoteMap {
    notes: HashMap<u32, String>,
}

#[allow(dead_code)]
impl EndnoteMap {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert(&mut self, id: u32, text: String) {
        self.notes.insert(id, text);
    }

    pub(crate) fn get(&self, id: u32) -> Option<&str> {
        self.notes.get(&id).map(|s| s.as_str())
    }

    pub(crate) fn len(&self) -> usize {
        self.notes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}

/// Parses `word/endnotes.xml` into an `EndnoteMap`. Separator and
/// continuationSeparator entries (those carrying a `w:type` attribute)
/// are skipped, same as for footnotes.
pub(crate) fn parse_endnotes_xml(xml_bytes: &[u8]) -> Result<EndnoteMap> {
    let notes = parse_note_collection(xml_bytes, "word/endnotes.xml", b"w:endnote")?;
    Ok(EndnoteMap { notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_endnotes_xml_yields_empty_map() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
</w:endnotes>"#;
        let map = parse_endnotes_xml(xml).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn user_endnote_concatenates_run_text_under_its_id() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:endnote w:id="1">
    <w:p><w:r><w:t>First </w:t></w:r><w:r><w:t>second.</w:t></w:r></w:p>
  </w:endnote>
</w:endnotes>"#;
        let map = parse_endnotes_xml(xml).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(1), Some("First second."));
    }

    #[test]
    fn separator_and_continuation_separator_endnotes_are_skipped() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:endnote w:id="-1" w:type="separator"><w:p><w:r><w:separator/></w:r></w:p></w:endnote>
  <w:endnote w:id="0" w:type="continuationSeparator"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:endnote>
  <w:endnote w:id="1"><w:p><w:r><w:t>real endnote</w:t></w:r></w:p></w:endnote>
</w:endnotes>"#;
        let map = parse_endnotes_xml(xml).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(1), Some("real endnote"));
    }

    #[test]
    fn multiple_user_endnotes_keep_distinct_entries() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:endnote w:id="1"><w:p><w:r><w:t>one</w:t></w:r></w:p></w:endnote>
  <w:endnote w:id="2"><w:p><w:r><w:t>two</w:t></w:r></w:p></w:endnote>
</w:endnotes>"#;
        let map = parse_endnotes_xml(xml).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(1), Some("one"));
        assert_eq!(map.get(2), Some("two"));
    }
}
