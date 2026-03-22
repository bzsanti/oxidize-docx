use std::path::Path;

use crate::error::{DocxError, Result};
use crate::ooxml::content_types::ContentTypeMap;
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

pub struct DocxDocument {
    #[allow(dead_code)]
    archive: SecureZipArchive,
    #[allow(dead_code)]
    content_types: ContentTypeMap,
}

impl DocxDocument {
    /// Opens a DOCX file and validates its structure.
    ///
    /// Performs security checks on the ZIP archive (entry count, sizes,
    /// path traversal) and parses the `[Content_Types].xml` manifest.
    ///
    /// Returns an error if:
    /// - The file cannot be opened (IO error)
    /// - The file is not a valid ZIP archive
    /// - The ZIP archive fails security validation
    /// - `[Content_Types].xml` is missing or invalid
    /// - The manifest does not declare a main document part
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut archive = SecureZipArchive::open(path.as_ref())?;

        let ct_bytes = archive
            .read_entry("[Content_Types].xml")
            .map_err(|e| match e {
                DocxError::MissingPart(_) => DocxError::MissingPart("[Content_Types].xml".into()),
                other => other,
            })?;

        let content_types = ContentTypeMap::parse(&ct_bytes)?;

        if content_types.main_document_part().is_none() {
            return Err(DocxError::MissingPart(
                "main document part (word/document.xml)".into(),
            ));
        }

        Ok(Self {
            archive,
            content_types,
        })
    }
}
