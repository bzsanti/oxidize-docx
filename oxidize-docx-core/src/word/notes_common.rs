use std::collections::HashMap;

use quick_xml::events::Event;

use crate::error::{DocxError, Result};
use crate::xml::reader::XmlReader;

/// Parses an OOXML notes collection (footnotes.xml or endnotes.xml) and
/// returns the user-authored entries keyed by `w:id`.
///
/// `note_tag` is the per-entry element name (`b"w:footnote"` or
/// `b"w:endnote"`); both formats share the same envelope and content
/// model so the parser is identical except for that tag. Entries
/// carrying a `w:type` attribute (`separator`, `continuationSeparator`)
/// are skipped — they exist only to render Word's visual divider and
/// would otherwise pollute downstream output with stray whitespace.
///
/// Text is read with `XmlReader::from_bytes_preserve_text` so trailing
/// spaces inside `<w:t>` survive run concatenation.
pub(crate) fn parse_note_collection(
    xml_bytes: &[u8],
    part_name: &str,
    note_tag: &[u8],
) -> Result<HashMap<u32, String>> {
    let mut reader = XmlReader::from_bytes_preserve_text(xml_bytes, part_name)?;
    let mut map: HashMap<u32, String> = HashMap::new();
    let mut buf = Vec::new();

    let mut in_note = false;
    let mut in_text = false;
    let mut current_id: Option<u32> = None;
    let mut current_skip = false;
    let mut current_text = String::new();

    loop {
        match reader.inner().read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local == note_tag {
                    in_note = true;
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
                } else if local == b"w:t" && in_note && !current_skip {
                    in_text = true;
                }
            }
            Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == note_tag {
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
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local == b"w:t" {
                    in_text = false;
                } else if local == note_tag && in_note {
                    if let (Some(id), false) = (current_id, current_skip) {
                        map.insert(id, current_text.clone());
                    }
                    in_note = false;
                    current_id = None;
                    current_skip = false;
                    current_text.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(DocxError::XmlParse {
                    part: part_name.to_string(),
                    reason: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(map)
}
