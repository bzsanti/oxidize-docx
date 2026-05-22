use std::collections::HashMap;

use quick_xml::events::Event;

use crate::error::{DocxError, Result};
use crate::xml::reader::XmlReader;

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
    let mut reader = XmlReader::from_bytes_preserve_text(xml_bytes, "word/footnotes.xml")?;
    let mut map = FootnoteMap::new();
    let mut buf = Vec::new();

    let mut in_footnote = false;
    let mut in_text = false;
    let mut current_id: Option<u32> = None;
    let mut current_skip = false;
    let mut current_text = String::new();

    loop {
        match reader.inner().read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"w:footnote" => {
                    in_footnote = true;
                    current_id = None;
                    current_skip = false;
                    current_text.clear();
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"w:id" => {
                                current_id = String::from_utf8_lossy(&attr.value)
                                    .parse::<i64>()
                                    .ok()
                                    .and_then(|i| u32::try_from(i).ok());
                            }
                            b"w:type" => {
                                current_skip = true;
                            }
                            _ => {}
                        }
                    }
                }
                b"w:t" if in_footnote && !current_skip => {
                    in_text = true;
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == b"w:footnote" {
                    let mut id = None;
                    let mut skip = false;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"w:id" => {
                                id = String::from_utf8_lossy(&attr.value)
                                    .parse::<i64>()
                                    .ok()
                                    .and_then(|i| u32::try_from(i).ok());
                            }
                            b"w:type" => skip = true,
                            _ => {}
                        }
                    }
                    if let (Some(id), false) = (id, skip) {
                        map.insert(id, String::new());
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_text => {
                current_text.push_str(&String::from_utf8_lossy(t));
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"w:t" => {
                    in_text = false;
                }
                b"w:footnote" if in_footnote => {
                    if let (Some(id), false) = (current_id, current_skip) {
                        map.insert(id, current_text.clone());
                    }
                    in_footnote = false;
                    current_id = None;
                    current_skip = false;
                    current_text.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: "word/footnotes.xml".into(),
                    reason: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(map)
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
