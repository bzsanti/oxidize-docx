use super::entity_guard::reject_if_doctype;
use crate::error::Result;

/// Thin wrapper over `quick_xml::Reader` that enforces security guards
/// (DOCTYPE rejection) before parsing begins.
pub(crate) struct XmlReader<'a> {
    reader: quick_xml::Reader<&'a [u8]>,
}

impl<'a> XmlReader<'a> {
    /// Creates a new `XmlReader` from raw XML bytes.
    ///
    /// Rejects XML that contains a DOCTYPE declaration before constructing
    /// the underlying quick-xml reader.
    pub(crate) fn from_bytes(bytes: &'a [u8], part_name: &str) -> Result<Self> {
        reject_if_doctype(bytes, part_name)?;

        let mut reader = quick_xml::Reader::from_reader(bytes);
        reader.config_mut().trim_text(true);

        Ok(Self { reader })
    }

    /// Returns a mutable reference to the underlying quick-xml reader.
    pub(crate) fn inner(&mut self) -> &mut quick_xml::Reader<&'a [u8]> {
        &mut self.reader
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bytes_accepts_clean_xml() {
        let xml = b"<root><child/></root>";
        let result = XmlReader::from_bytes(xml, "test.xml");
        assert!(result.is_ok());
    }

    #[test]
    fn from_bytes_rejects_doctype_xml() {
        let xml = b"<!DOCTYPE bomb><root/>";
        let result = XmlReader::from_bytes(xml, "evil.xml");
        assert!(result.is_err());
    }

    #[test]
    fn reader_can_parse_events() {
        let xml = b"<root><child>text</child></root>";
        let mut reader = XmlReader::from_bytes(xml, "test.xml").unwrap();
        let mut buf = Vec::new();
        let event = reader.inner().read_event_into(&mut buf).unwrap();
        assert!(matches!(event, quick_xml::events::Event::Start(_)));
    }
}
