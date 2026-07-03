//! Metadata helpers for RAG chunks: content-type flags, deterministic
//! chunk ids, and prev/next linking. Mirrors the applicable subset of
//! oxidize-pdf's `pipeline/chunk_metadata.rs` (no font/coordinate/page
//! metadata — docx has no source for those).

use sha2::{Digest, Sha256};

use crate::pipeline::element::HeadingContext;

/// Coarse flags describing what an emitted chunk contains, so downstream
/// rerankers can route (e.g. tables to a table-QA path) without re-parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContentTypeFlags {
    pub has_table: bool,
    pub has_list: bool,
    /// True when the chunk holds only a heading and no body content.
    pub heading_only: bool,
}

/// Derives content flags from a chunk's `element_types` tags.
pub(crate) fn content_type_flags(element_types: &[String]) -> ContentTypeFlags {
    let has_table = element_types.iter().any(|t| t == "table");
    let has_list = element_types.iter().any(|t| t == "list_item");
    let heading_only = !element_types.is_empty() && element_types.iter().all(|t| t == "heading");
    ContentTypeFlags {
        has_table,
        has_list,
        heading_only,
    }
}

/// Flattens a heading context stack (root→leaf) to its text breadcrumb.
pub(crate) fn heading_path_from(ctx: &[HeadingContext]) -> Vec<String> {
    ctx.iter().map(|h| h.text.clone()).collect()
}

/// Deterministic chunk id `"{doc_id}:{index}"`. `doc_id` is the first 8 bytes
/// of SHA-256 over the chunk's `full_text`, hex-encoded (16 chars). Same
/// content always yields the same id — stable across runs and machines.
pub(crate) fn compute_chunk_id(full_text: &str, index: usize) -> String {
    let digest = Sha256::digest(full_text.as_bytes());
    let mut doc_id = String::with_capacity(16);
    for byte in &digest[..8] {
        doc_id.push_str(&format!("{byte:02x}"));
    }
    format!("{doc_id}:{index}")
}
