use std::collections::HashMap;

use crate::error::Result;
use crate::word::notes_common::parse_note_collection;

/// Map of `w:footnote w:id="N"` → concatenated text of the footnote's runs.
/// Separator and continuationSeparator footnotes (those carrying a `w:type`
/// attribute) are filtered out at parse time so the map only contains
/// user-authored footnotes ready for downstream classification.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct FootnoteMap {
    notes: HashMap<u32, String>,
}

#[allow(dead_code)]
impl FootnoteMap {
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

/// Parses `word/footnotes.xml` into a `FootnoteMap`. Footnotes whose
/// `w:footnote` element carries a `w:type` attribute (`separator`,
/// `continuationSeparator`) are not user-authored content and are
/// skipped — they exist only to render the visual divider in Word's
/// page-layout view.
pub(crate) fn parse_footnotes_xml(xml_bytes: &[u8]) -> Result<FootnoteMap> {
    let notes = parse_note_collection(xml_bytes, "word/footnotes.xml", b"w:footnote")?;
    Ok(FootnoteMap { notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_footnotes_xml_yields_empty_map() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
</w:footnotes>"#;
        let map = parse_footnotes_xml(xml).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn user_footnote_concatenates_run_text_under_its_id() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="1">
    <w:p><w:r><w:t>First </w:t></w:r><w:r><w:t>second.</w:t></w:r></w:p>
  </w:footnote>
</w:footnotes>"#;
        let map = parse_footnotes_xml(xml).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(1), Some("First second."));
    }

    #[test]
    fn separator_and_continuation_separator_footnotes_are_skipped() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="-1" w:type="separator">
    <w:p><w:r><w:separator/></w:r></w:p>
  </w:footnote>
  <w:footnote w:id="0" w:type="continuationSeparator">
    <w:p><w:r><w:continuationSeparator/></w:r></w:p>
  </w:footnote>
  <w:footnote w:id="1">
    <w:p><w:r><w:t>real note</w:t></w:r></w:p>
  </w:footnote>
</w:footnotes>"#;
        let map = parse_footnotes_xml(xml).unwrap();
        assert_eq!(map.len(), 1, "only user-authored footnote survives");
        assert_eq!(map.get(1), Some("real note"));
    }

    #[test]
    fn multiple_user_footnotes_keep_distinct_entries() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="1"><w:p><w:r><w:t>first</w:t></w:r></w:p></w:footnote>
  <w:footnote w:id="2"><w:p><w:r><w:t>second</w:t></w:r></w:p></w:footnote>
</w:footnotes>"#;
        let map = parse_footnotes_xml(xml).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(1), Some("first"));
        assert_eq!(map.get(2), Some("second"));
    }

    #[test]
    fn unknown_id_returns_none() {
        let xml = br#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="1"><w:p><w:r><w:t>x</w:t></w:r></w:p></w:footnote>
</w:footnotes>"#;
        let map = parse_footnotes_xml(xml).unwrap();
        assert!(map.get(42).is_none());
    }
}
