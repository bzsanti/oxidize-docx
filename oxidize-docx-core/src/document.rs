use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use crate::error::{DocxError, Result};
use crate::images::extractor::extract_images;
use crate::images::ImageMetadata;
use crate::numbering::defs::NumberingDefs;
use crate::ooxml::content_types::ContentTypeMap;
use crate::ooxml::relationships::RelationshipMap;
use crate::pipeline::export::{to_markdown, to_plain_text};
use crate::pipeline::profile::ExtractionProfile;
use crate::pipeline::rag::{DocxRagChunker, RagChunk};
use crate::pipeline::{ClassifierPipeline, DocxElement};
use crate::raw::body::RawBody;
use crate::styles::table::StyleTable;
use crate::word::comments_xml::{parse_comments_xml, CommentMap};
use crate::word::document_xml::{parse_document_xml, parse_footer_xml, parse_header_xml};
use crate::word::endnotes_xml::{parse_endnotes_xml, EndnoteMap};
use crate::word::footnotes_xml::{parse_footnotes_xml, FootnoteMap};
use crate::word::numbering_xml::parse_numbering_xml;
use crate::word::styles_xml::parse_styles_xml;
use crate::zip::SecureZipArchive;

/// Main entry point for parsing DOCX documents.
///
/// Opens a DOCX file, validates its ZIP structure and OOXML manifests,
/// and provides methods to extract content (elements, RAG chunks, etc.).
///
/// # Example (Phase 1 — open only)
/// ```no_run
/// use oxidize_docx::DocxDocument;
/// let doc = DocxDocument::open("report.docx").unwrap();
/// ```
impl std::fmt::Debug for DocxDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocxDocument").finish_non_exhaustive()
    }
}

/// Raw XML bytes extracted from the ZIP archive during `open()`.
///
/// Stored eagerly so we don't need mutable access to the archive
/// after construction. Parsed lazily on demand via RefCell caching.
#[allow(dead_code)]
struct RawXmlParts {
    document_xml: Vec<u8>,
    styles_xml: Option<Vec<u8>>,
    numbering_xml: Option<Vec<u8>>,
    footnotes_xml: Option<Vec<u8>>,
    endnotes_xml: Option<Vec<u8>>,
    comments_xml: Option<Vec<u8>>,
    document_rels_xml: Option<Vec<u8>>,
    /// All `word/headerN.xml` parts present in the archive, keyed by
    /// their full archive path (e.g., `"word/header1.xml"`). The
    /// classifier looks them up by resolving a `<w:headerReference>`'s
    /// `rel_id` to a `Target` via the document's relationships and
    /// then joining `"word/"` + target.
    header_parts: HashMap<String, Vec<u8>>,
    /// Same as `header_parts` but for footers (`word/footerN.xml`).
    footer_parts: HashMap<String, Vec<u8>>,
}

#[allow(dead_code)]
pub struct DocxDocument {
    content_types: ContentTypeMap,
    raw_parts: RawXmlParts,
    archive: RefCell<SecureZipArchive>,
    body_cache: RefCell<Option<RawBody>>,
    styles_cache: RefCell<Option<Option<StyleTable>>>,
    numbering_cache: RefCell<Option<Option<NumberingDefs>>>,
    footnotes_cache: RefCell<Option<Option<FootnoteMap>>>,
    endnotes_cache: RefCell<Option<Option<EndnoteMap>>>,
    comments_cache: RefCell<Option<Option<CommentMap>>>,
    document_rels_cache: RefCell<Option<Option<RelationshipMap>>>,
    elements_cache: RefCell<Option<Vec<DocxElement>>>,
    /// Parsed header parts keyed by archive path. Built lazily once on
    /// first access — empty when the document declared no headers.
    header_bodies_cache: RefCell<Option<HashMap<String, RawBody>>>,
    footer_bodies_cache: RefCell<Option<HashMap<String, RawBody>>>,
}

impl DocxDocument {
    /// Opens a DOCX file and validates its structure.
    ///
    /// Performs security checks on the ZIP archive (entry count, sizes,
    /// path traversal) and parses the `[Content_Types].xml` manifest.
    /// Eagerly reads raw XML bytes for the main document part and
    /// optional styles/numbering parts.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut archive = SecureZipArchive::open(path.as_ref())?;

        let ct_bytes = archive
            .read_entry("[Content_Types].xml")
            .map_err(|e| match e {
                DocxError::MissingPart(_) => DocxError::MissingPart("[Content_Types].xml".into()),
                other => other,
            })?;

        let content_types = ContentTypeMap::parse(&ct_bytes)?;

        let doc_part = content_types.main_document_part().ok_or_else(|| {
            DocxError::MissingPart("main document part (word/document.xml)".into())
        })?;

        let document_xml = archive.read_entry(&doc_part)?;

        // Styles, numbering and footnotes are optional
        let styles_xml = archive.read_entry("word/styles.xml").ok();
        let numbering_xml = archive.read_entry("word/numbering.xml").ok();
        let footnotes_xml = archive.read_entry("word/footnotes.xml").ok();
        let endnotes_xml = archive.read_entry("word/endnotes.xml").ok();
        let comments_xml = archive.read_entry("word/comments.xml").ok();
        let document_rels_xml = archive.read_entry("word/_rels/document.xml.rels").ok();

        // Eagerly read every word/header*.xml and word/footer*.xml part
        // present in the archive. Filenames are not always header1.xml /
        // footer1.xml — Word numbers them by reference order, with gaps
        // when a slot is reused — so we scan rather than guess.
        let entry_names = archive.entry_names();
        let mut header_parts: HashMap<String, Vec<u8>> = HashMap::new();
        let mut footer_parts: HashMap<String, Vec<u8>> = HashMap::new();
        for name in &entry_names {
            if is_header_part_path(name) {
                if let Ok(bytes) = archive.read_entry(name) {
                    header_parts.insert(name.clone(), bytes);
                }
            } else if is_footer_part_path(name) {
                if let Ok(bytes) = archive.read_entry(name) {
                    footer_parts.insert(name.clone(), bytes);
                }
            }
        }

        Ok(Self {
            content_types,
            raw_parts: RawXmlParts {
                document_xml,
                styles_xml,
                numbering_xml,
                footnotes_xml,
                endnotes_xml,
                comments_xml,
                document_rels_xml,
                header_parts,
                footer_parts,
            },
            archive: RefCell::new(archive),
            body_cache: RefCell::new(None),
            styles_cache: RefCell::new(None),
            numbering_cache: RefCell::new(None),
            footnotes_cache: RefCell::new(None),
            endnotes_cache: RefCell::new(None),
            comments_cache: RefCell::new(None),
            document_rels_cache: RefCell::new(None),
            elements_cache: RefCell::new(None),
            header_bodies_cache: RefCell::new(None),
            footer_bodies_cache: RefCell::new(None),
        })
    }

    /// Returns the parsed raw body of the document.
    ///
    /// Parses `word/document.xml` on first call, then caches the result.
    #[allow(dead_code)]
    pub(crate) fn raw_body(&self) -> Result<std::cell::Ref<'_, RawBody>> {
        // Parse and cache if not already done
        {
            let mut cache = self.body_cache.borrow_mut();
            if cache.is_none() {
                let body = parse_document_xml(&self.raw_parts.document_xml)?;
                *cache = Some(body);
            }
        }

        Ok(std::cell::Ref::map(self.body_cache.borrow(), |opt| {
            opt.as_ref().expect("body_cache should be populated")
        }))
    }

    /// Returns the parsed style table, if `word/styles.xml` exists.
    ///
    /// Returns `Ok(None)` if the document has no styles part.
    #[allow(dead_code)]
    pub(crate) fn style_table(&self) -> Result<Option<std::cell::Ref<'_, StyleTable>>> {
        {
            let mut cache = self.styles_cache.borrow_mut();
            if cache.is_none() {
                let parsed = match &self.raw_parts.styles_xml {
                    Some(bytes) => Some(parse_styles_xml(bytes)?),
                    None => None,
                };
                *cache = Some(parsed);
            }
        }

        let borrowed = self.styles_cache.borrow();
        // If the inner Option is None (no styles.xml), return Ok(None)
        if borrowed.as_ref().expect("cache populated").is_none() {
            return Ok(None);
        }

        Ok(Some(std::cell::Ref::map(borrowed, |opt| {
            opt.as_ref()
                .expect("cache populated")
                .as_ref()
                .expect("styles present")
        })))
    }

    /// Returns the parsed numbering definitions, if `word/numbering.xml` exists.
    ///
    /// Returns `Ok(None)` if the document has no numbering part.
    #[allow(dead_code)]
    pub(crate) fn numbering_defs(&self) -> Result<Option<std::cell::Ref<'_, NumberingDefs>>> {
        {
            let mut cache = self.numbering_cache.borrow_mut();
            if cache.is_none() {
                let parsed = match &self.raw_parts.numbering_xml {
                    Some(bytes) => Some(parse_numbering_xml(bytes)?),
                    None => None,
                };
                *cache = Some(parsed);
            }
        }

        let borrowed = self.numbering_cache.borrow();
        if borrowed.as_ref().expect("cache populated").is_none() {
            return Ok(None);
        }

        Ok(Some(std::cell::Ref::map(borrowed, |opt| {
            opt.as_ref()
                .expect("cache populated")
                .as_ref()
                .expect("numbering present")
        })))
    }

    /// Returns the parsed footnote map, if `word/footnotes.xml` exists.
    ///
    /// Returns `Ok(None)` if the document has no footnotes part.
    #[allow(dead_code)]
    pub(crate) fn footnotes(&self) -> Result<Option<std::cell::Ref<'_, FootnoteMap>>> {
        {
            let mut cache = self.footnotes_cache.borrow_mut();
            if cache.is_none() {
                let parsed = match &self.raw_parts.footnotes_xml {
                    Some(bytes) => Some(parse_footnotes_xml(bytes)?),
                    None => None,
                };
                *cache = Some(parsed);
            }
        }

        let borrowed = self.footnotes_cache.borrow();
        if borrowed.as_ref().expect("cache populated").is_none() {
            return Ok(None);
        }

        Ok(Some(std::cell::Ref::map(borrowed, |opt| {
            opt.as_ref()
                .expect("cache populated")
                .as_ref()
                .expect("footnotes present")
        })))
    }

    /// Returns the parsed endnote map, if `word/endnotes.xml` exists.
    ///
    /// Returns `Ok(None)` if the document has no endnotes part.
    #[allow(dead_code)]
    pub(crate) fn endnotes(&self) -> Result<Option<std::cell::Ref<'_, EndnoteMap>>> {
        {
            let mut cache = self.endnotes_cache.borrow_mut();
            if cache.is_none() {
                let parsed = match &self.raw_parts.endnotes_xml {
                    Some(bytes) => Some(parse_endnotes_xml(bytes)?),
                    None => None,
                };
                *cache = Some(parsed);
            }
        }

        let borrowed = self.endnotes_cache.borrow();
        if borrowed.as_ref().expect("cache populated").is_none() {
            return Ok(None);
        }

        Ok(Some(std::cell::Ref::map(borrowed, |opt| {
            opt.as_ref()
                .expect("cache populated")
                .as_ref()
                .expect("endnotes present")
        })))
    }

    /// Returns the parsed comment map, if `word/comments.xml` exists.
    ///
    /// Returns `Ok(None)` if the document has no comments part.
    #[allow(dead_code)]
    pub(crate) fn comments(&self) -> Result<Option<std::cell::Ref<'_, CommentMap>>> {
        {
            let mut cache = self.comments_cache.borrow_mut();
            if cache.is_none() {
                let parsed = match &self.raw_parts.comments_xml {
                    Some(bytes) => Some(parse_comments_xml(bytes)?),
                    None => None,
                };
                *cache = Some(parsed);
            }
        }

        let borrowed = self.comments_cache.borrow();
        if borrowed.as_ref().expect("cache populated").is_none() {
            return Ok(None);
        }

        Ok(Some(std::cell::Ref::map(borrowed, |opt| {
            opt.as_ref()
                .expect("cache populated")
                .as_ref()
                .expect("comments present")
        })))
    }

    /// Parses every header part on first call and caches the result.
    /// Returns the (possibly empty) map keyed by archive path.
    #[allow(dead_code)]
    pub(crate) fn header_bodies(&self) -> Result<std::cell::Ref<'_, HashMap<String, RawBody>>> {
        {
            let mut cache = self.header_bodies_cache.borrow_mut();
            if cache.is_none() {
                let mut map: HashMap<String, RawBody> = HashMap::new();
                for (path, bytes) in &self.raw_parts.header_parts {
                    map.insert(path.clone(), parse_header_xml(bytes)?);
                }
                *cache = Some(map);
            }
        }
        Ok(std::cell::Ref::map(
            self.header_bodies_cache.borrow(),
            |opt| opt.as_ref().expect("header bodies cache populated"),
        ))
    }

    /// Same as `header_bodies` but for footers.
    #[allow(dead_code)]
    pub(crate) fn footer_bodies(&self) -> Result<std::cell::Ref<'_, HashMap<String, RawBody>>> {
        {
            let mut cache = self.footer_bodies_cache.borrow_mut();
            if cache.is_none() {
                let mut map: HashMap<String, RawBody> = HashMap::new();
                for (path, bytes) in &self.raw_parts.footer_parts {
                    map.insert(path.clone(), parse_footer_xml(bytes)?);
                }
                *cache = Some(map);
            }
        }
        Ok(std::cell::Ref::map(
            self.footer_bodies_cache.borrow(),
            |opt| opt.as_ref().expect("footer bodies cache populated"),
        ))
    }

    /// Returns the raw bytes of a header part by archive path
    /// (`"word/header1.xml"` etc.). The classifier uses this when
    /// resolving a `<w:headerReference>`: relationships map gives the
    /// Target, this method gives the bytes.
    #[allow(dead_code)]
    pub(crate) fn header_part(&self, path: &str) -> Option<&[u8]> {
        self.raw_parts.header_parts.get(path).map(|v| v.as_slice())
    }

    /// Returns the raw bytes of a footer part by archive path. See
    /// `header_part` for the resolution flow.
    #[allow(dead_code)]
    pub(crate) fn footer_part(&self, path: &str) -> Option<&[u8]> {
        self.raw_parts.footer_parts.get(path).map(|v| v.as_slice())
    }

    /// Returns the parsed relationship map for the main document part,
    /// if `word/_rels/document.xml.rels` exists. The map resolves
    /// `r:id` attributes (hyperlinks, images, headers/footers…) to
    /// their concrete `Target`.
    ///
    /// Returns `Ok(None)` when the document declares no rels for its
    /// main part — that's a perfectly valid (link-less, image-less)
    /// document and must not be an error.
    #[allow(dead_code)]
    pub(crate) fn document_relationships(
        &self,
    ) -> Result<Option<std::cell::Ref<'_, RelationshipMap>>> {
        {
            let mut cache = self.document_rels_cache.borrow_mut();
            if cache.is_none() {
                let parsed = match &self.raw_parts.document_rels_xml {
                    Some(bytes) => Some(RelationshipMap::parse(
                        bytes,
                        "word/_rels/document.xml.rels",
                    )?),
                    None => None,
                };
                *cache = Some(parsed);
            }
        }

        let borrowed = self.document_rels_cache.borrow();
        if borrowed.as_ref().expect("cache populated").is_none() {
            return Ok(None);
        }

        Ok(Some(std::cell::Ref::map(borrowed, |opt| {
            opt.as_ref()
                .expect("cache populated")
                .as_ref()
                .expect("rels present")
        })))
    }

    /// Classifies the document's raw body into semantic `DocxElement`s.
    ///
    /// Builds (and discards) a transient `ClassifierPipeline` per call. The
    /// raw body, styles table, and numbering definitions are parsed lazily
    /// the first time any of them is needed and cached on `self`, so a second
    /// call only re-runs the classifier — not the XML parsers. If
    /// `word/styles.xml` or `word/numbering.xml` are absent, empty defaults
    /// stand in for them so styleless or list-less documents classify
    /// successfully.
    pub fn elements(&self) -> Result<Vec<DocxElement>> {
        // Cache hit: clone the previously-classified vector and return.
        // `clone` is cheap relative to the full classifier run (XML walk +
        // style/numbering resolution), and keeping the cache by-value lets
        // the public API stay `Result<Vec<_>>` instead of leaking a Ref.
        if let Some(ref cached) = *self.elements_cache.borrow() {
            return Ok(cached.clone());
        }

        let body = self.raw_body()?;
        let style_ref = self.style_table()?;
        let numbering_ref = self.numbering_defs()?;
        let footnotes_ref = self.footnotes()?;
        let endnotes_ref = self.endnotes()?;
        let comments_ref = self.comments()?;
        let rels_ref = self.document_relationships()?;
        let header_bodies_ref = self.header_bodies()?;
        let footer_bodies_ref = self.footer_bodies()?;

        let empty_styles = StyleTable::new();
        let empty_numbering = NumberingDefs::new();
        let styles: &StyleTable = style_ref.as_deref().unwrap_or(&empty_styles);
        let numbering: &NumberingDefs = numbering_ref.as_deref().unwrap_or(&empty_numbering);

        let mut classifier = ClassifierPipeline::new(styles, numbering);
        if let Some(ref fn_ref) = footnotes_ref {
            classifier = classifier.with_footnotes(fn_ref);
        }
        if let Some(ref en_ref) = endnotes_ref {
            classifier = classifier.with_endnotes(en_ref);
        }
        if let Some(ref cm_ref) = comments_ref {
            classifier = classifier.with_comments(cm_ref);
        }
        if let Some(ref r_ref) = rels_ref {
            classifier = classifier.with_relationships(r_ref);
        }
        classifier = classifier.with_section_bodies(&header_bodies_ref, &footer_bodies_ref);
        let classified = classifier.classify(&body)?;

        *self.elements_cache.borrow_mut() = Some(classified.clone());
        Ok(classified)
    }

    /// Renders the document as unformatted plain text. Blocks are separated
    /// by a blank line; consecutive list items use a single newline; table
    /// cells in a row are joined by ` | `.
    pub fn plain_text(&self) -> Result<String> {
        let elements = self.elements()?;
        Ok(to_plain_text(&elements))
    }

    /// Renders the document as GitHub-flavored Markdown. Headings use
    /// `#` prefixes (clamped to depth 6), paragraphs flow as plain text,
    /// list items indent by 2 spaces per level with `N.` for decimal
    /// lists or `-` for bullets, and tables use the GFM pipe syntax
    /// with row 0 as the header.
    pub fn to_markdown(&self) -> Result<String> {
        let elements = self.elements()?;
        Ok(to_markdown(&elements))
    }

    /// Returns the document chunked for ingestion into a RAG pipeline,
    /// using the default `DocxRagChunker` (max 800 tokens per chunk).
    /// Each chunk carries its `heading_context`, `paragraph_indices`
    /// (positions in the `elements()` output), `element_types`, and an
    /// `is_oversized` flag set when the source content had to be split
    /// at sentence boundaries to fit.
    pub fn rag_chunks(&self) -> Result<Vec<RagChunk>> {
        let elements = self.elements()?;
        Ok(DocxRagChunker::new().chunk(&elements))
    }

    /// Same as `rag_chunks()` but lets the caller pick an
    /// `ExtractionProfile`: `Minimal` drops footnotes/endnotes/comments
    /// before chunking, `Academic` inlines note text into the
    /// referencing paragraph, `Default` / `Technical` keep every
    /// element as-is.
    pub fn rag_chunks_with_profile(&self, profile: ExtractionProfile) -> Result<Vec<RagChunk>> {
        let elements = self.elements()?;
        Ok(DocxRagChunker::new().with_profile(profile).chunk(&elements))
    }

    /// Extracts all raster images embedded under `word/media/` in the
    /// archive, sniffing each one's content type from its magic bytes.
    /// The result is sorted by path so iteration order is deterministic
    /// across runs.
    pub fn images(&self) -> Result<Vec<ImageMetadata>> {
        let mut archive = self.archive.borrow_mut();
        extract_images(&mut archive)
    }
}

/// Matches `word/header<digits>.xml` exactly. The "word/" prefix and
/// `.xml` suffix are required; the middle must be `"header"` followed
/// by one or more ASCII digits.
fn is_header_part_path(name: &str) -> bool {
    matches_part(name, "header")
}

/// Matches `word/footer<digits>.xml` exactly. See `is_header_part_path`.
fn is_footer_part_path(name: &str) -> bool {
    matches_part(name, "footer")
}

fn matches_part(name: &str, kind: &str) -> bool {
    let Some(rest) = name.strip_prefix("word/") else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(kind) else {
        return false;
    };
    let Some(digits) = rest.strip_suffix(".xml") else {
        return false;
    };
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod part_path_tests {
    use super::*;

    #[test]
    fn matches_word_header_n_xml_with_digits() {
        assert!(is_header_part_path("word/header1.xml"));
        assert!(is_header_part_path("word/header42.xml"));
        assert!(is_footer_part_path("word/footer1.xml"));
    }

    #[test]
    fn rejects_paths_without_digits_or_outside_word_dir() {
        assert!(!is_header_part_path("word/header.xml"));
        assert!(!is_header_part_path("word/headers.xml"));
        assert!(!is_header_part_path("docs/header1.xml"));
        assert!(!is_header_part_path("word/header1.txt"));
        assert!(!is_footer_part_path("word/footnote1.xml"));
    }
}
