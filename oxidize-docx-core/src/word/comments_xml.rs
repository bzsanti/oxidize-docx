use std::collections::HashMap;

use quick_xml::events::Event;

use crate::error::{DocxError, Result};
use crate::xml::reader::XmlReader;

/// Metadata + body text for a single `<w:comment>` entry. The author
/// string is captured verbatim from the `w:author` attribute; date and
/// initials are intentionally dropped at this layer — they belong in a
/// future `CommentInfoExt` if a downstream consumer needs them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub(crate) struct CommentInfo {
    pub(crate) author: String,
    pub(crate) text: String,
}

/// Map of `w:comment w:id="N"` → `CommentInfo`. Unlike footnotes, the
/// comments part has no separator/continuationSeparator entries to
/// filter — every `<w:comment>` is user-authored.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct CommentMap {
    comments: HashMap<u32, CommentInfo>,
}

#[allow(dead_code)]
impl CommentMap {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn insert(&mut self, id: u32, info: CommentInfo) {
        self.comments.insert(id, info);
    }

    pub(crate) fn get(&self, id: u32) -> Option<&CommentInfo> {
        self.comments.get(&id)
    }

    pub(crate) fn len(&self) -> usize {
        self.comments.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.comments.is_empty()
    }
}

/// Parses `word/comments.xml`. Each `<w:comment>` element contributes one
/// entry keyed by `w:id`; author is read from `w:author`, and the body
/// text is concatenated from every `<w:t>` descendant in order. Text is
/// read with `preserve_text` so spaces between runs survive.
pub(crate) fn parse_comments_xml(xml_bytes: &[u8]) -> Result<CommentMap> {
    let mut reader = XmlReader::from_bytes_preserve_text(xml_bytes, "word/comments.xml")?;
    let mut map = CommentMap::new();
    let mut buf = Vec::new();

    let mut in_comment = false;
    let mut in_text = false;
    let mut current_id: Option<u32> = None;
    let mut current_author = String::new();
    let mut current_text = String::new();

    loop {
        match reader.inner().read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local == b"w:comment" {
                    in_comment = true;
                    current_id = None;
                    current_author.clear();
                    current_text.clear();
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"w:id" => {
                                current_id = String::from_utf8_lossy(&attr.value)
                                    .parse::<i64>()
                                    .ok()
                                    .and_then(|i| u32::try_from(i).ok());
                            }
                            b"w:author" => {
                                current_author = String::from_utf8_lossy(&attr.value).into_owned();
                            }
                            _ => {}
                        }
                    }
                } else if local == b"w:t" && in_comment {
                    in_text = true;
                }
            }
            Ok(Event::Text(ref t)) if in_text => {
                current_text.push_str(&String::from_utf8_lossy(t));
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local == b"w:t" {
                    in_text = false;
                } else if local == b"w:comment" && in_comment {
                    if let Some(id) = current_id {
                        map.insert(
                            id,
                            CommentInfo {
                                author: std::mem::take(&mut current_author),
                                text: std::mem::take(&mut current_text),
                            },
                        );
                    }
                    in_comment = false;
                    current_id = None;
                    current_author.clear();
                    current_text.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: "word/comments.xml".into(),
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
    fn empty_comments_xml_yields_empty_map() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
</w:comments>"#;
        let map = parse_comments_xml(xml).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn single_comment_captures_author_and_text() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:comment w:id="0" w:author="Jane Doe" w:date="2026-05-22T12:00:00Z" w:initials="JD">
    <w:p><w:r><w:t>Please clarify </w:t></w:r><w:r><w:t>this term.</w:t></w:r></w:p>
  </w:comment>
</w:comments>"#;
        let map = parse_comments_xml(xml).unwrap();
        assert_eq!(map.len(), 1);
        let info = map.get(0).expect("comment 0 present");
        assert_eq!(info.author, "Jane Doe");
        assert_eq!(info.text, "Please clarify this term.");
    }

    #[test]
    fn multiple_comments_keep_distinct_entries() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:comment w:id="1" w:author="A"><w:p><w:r><w:t>first</w:t></w:r></w:p></w:comment>
  <w:comment w:id="2" w:author="B"><w:p><w:r><w:t>second</w:t></w:r></w:p></w:comment>
</w:comments>"#;
        let map = parse_comments_xml(xml).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(1).unwrap().author, "A");
        assert_eq!(map.get(1).unwrap().text, "first");
        assert_eq!(map.get(2).unwrap().author, "B");
        assert_eq!(map.get(2).unwrap().text, "second");
    }

    #[test]
    fn missing_author_attribute_yields_empty_string() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:comment w:id="5"><w:p><w:r><w:t>anonymous</w:t></w:r></w:p></w:comment>
</w:comments>"#;
        let map = parse_comments_xml(xml).unwrap();
        let info = map.get(5).unwrap();
        assert_eq!(info.author, "");
        assert_eq!(info.text, "anonymous");
    }
}
