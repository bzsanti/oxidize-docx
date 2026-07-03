//! Metadata helpers for RAG chunks: content-type flags, deterministic
//! chunk ids, and prev/next linking. Mirrors the applicable subset of
//! oxidize-pdf's `pipeline/chunk_metadata.rs` (no font/coordinate/page
//! metadata — docx has no source for those).

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
