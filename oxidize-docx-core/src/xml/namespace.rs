#![allow(dead_code)]

/// WordprocessingML main namespace (w:)
pub const WORDPROCESSINGML: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Package relationships namespace
pub const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Content types namespace
pub const CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// DrawingML main namespace (a:)
pub const DRAWING_ML: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// Office document relationships namespace (used in relationship Type attributes)
pub const RELATIONSHIPS_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordprocessingml_namespace_is_correct() {
        assert_eq!(
            WORDPROCESSINGML,
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
        );
    }

    #[test]
    fn relationships_namespace_is_correct() {
        assert!(RELATIONSHIPS.contains("relationships"));
    }

    #[test]
    fn content_types_namespace_is_correct() {
        assert!(CONTENT_TYPES.contains("content-types"));
    }

    #[test]
    fn drawing_ml_namespace_is_correct() {
        assert!(DRAWING_ML.contains("drawingml"));
    }

    #[test]
    fn relationships_document_namespace_is_correct() {
        assert!(RELATIONSHIPS_DOCUMENT.contains("officeDocument"));
    }
}
