use std::cell::RefCell;
use std::path::Path;

use crate::error::{DocxError, Result};
use crate::numbering::defs::NumberingDefs;
use crate::ooxml::content_types::ContentTypeMap;
use crate::pipeline::export::{to_markdown, to_plain_text};
use crate::pipeline::rag::{DocxRagChunker, RagChunk};
use crate::pipeline::{ClassifierPipeline, DocxElement};
use crate::raw::body::RawBody;
use crate::styles::table::StyleTable;
use crate::word::document_xml::parse_document_xml;
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
}

#[allow(dead_code)]
pub struct DocxDocument {
    content_types: ContentTypeMap,
    raw_parts: RawXmlParts,
    body_cache: RefCell<Option<RawBody>>,
    styles_cache: RefCell<Option<Option<StyleTable>>>,
    numbering_cache: RefCell<Option<Option<NumberingDefs>>>,
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

        // Styles and numbering are optional
        let styles_xml = archive.read_entry("word/styles.xml").ok();
        let numbering_xml = archive.read_entry("word/numbering.xml").ok();

        Ok(Self {
            content_types,
            raw_parts: RawXmlParts {
                document_xml,
                styles_xml,
                numbering_xml,
            },
            body_cache: RefCell::new(None),
            styles_cache: RefCell::new(None),
            numbering_cache: RefCell::new(None),
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
        let body = self.raw_body()?;
        let style_ref = self.style_table()?;
        let numbering_ref = self.numbering_defs()?;

        let empty_styles = StyleTable::new();
        let empty_numbering = NumberingDefs::new();
        let styles: &StyleTable = style_ref.as_deref().unwrap_or(&empty_styles);
        let numbering: &NumberingDefs = numbering_ref.as_deref().unwrap_or(&empty_numbering);

        let mut classifier = ClassifierPipeline::new(styles, numbering);
        classifier.classify(&body)
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
}
