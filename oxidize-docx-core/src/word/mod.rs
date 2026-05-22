pub(crate) mod document_xml;
pub(crate) mod footnotes_xml;
pub(crate) mod numbering_xml;
pub(crate) mod styles_xml;

#[allow(unused_imports)]
pub(crate) use document_xml::parse_document_xml;
#[allow(unused_imports)]
pub(crate) use footnotes_xml::{parse_footnotes_xml, FootnoteMap};
#[allow(unused_imports)]
pub(crate) use numbering_xml::parse_numbering_xml;
#[allow(unused_imports)]
pub(crate) use styles_xml::parse_styles_xml;
