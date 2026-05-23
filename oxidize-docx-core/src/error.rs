use thiserror::Error;

#[derive(Error, Debug)]
pub enum DocxError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error(
        "ZIP bomb detected: entry '{entry}' claims {claimed_size} bytes (limit {limit} bytes)"
    )]
    ZipBomb {
        entry: String,
        claimed_size: u64,
        limit: u64,
    },

    #[error("ZIP entry count exceeds safety limit: {count} entries (limit {limit})")]
    ZipEntryLimitExceeded { count: usize, limit: usize },

    #[error("ZIP path traversal detected: '{path}'")]
    ZipPathTraversal { path: String },

    #[error("Missing required OOXML part: '{0}'")]
    MissingPart(String),

    #[error("Invalid content types manifest: {0}")]
    InvalidContentTypes(String),

    #[error("Invalid relationships file '{path}': {reason}")]
    InvalidRelationships { path: String, reason: String },

    #[error("XML parse error in '{part}': {reason}")]
    XmlParse { part: String, reason: String },

    #[error("XML entity expansion limit exceeded in '{part}' (limit: {limit} expansions)")]
    XmlEntityExpansionLimit { part: String, limit: usize },

    #[error("Unexpected XML element '{element}' in '{context}'")]
    UnexpectedElement { element: String, context: String },

    #[error("Style '{0}' references itself in basedOn chain (circular reference)")]
    CircularStyleReference(String),

    #[error("Style chain depth exceeded for style '{style}' (limit {limit})")]
    StyleChainTooDeep { style: String, limit: usize },

    #[error("Numbering definition '{num_id}' not found")]
    NumberingDefNotFound { num_id: u32 },

    #[error("Abstract numbering '{abstract_num_id}' not found")]
    AbstractNumNotFound { abstract_num_id: u32 },

    #[error("Image relationship '{rel_id}' not found in document relationships")]
    ImageRelNotFound { rel_id: String },

    #[error("Image part '{path}' not found in ZIP archive")]
    ImagePartNotFound { path: String },

    #[error("Pipeline error: {0}")]
    Pipeline(String),

    #[error("Text encoding error in part '{part}': {reason}")]
    Encoding { part: String, reason: String },
}

pub type Result<T> = std::result::Result<T, DocxError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_zip_bomb() {
        let e = DocxError::ZipBomb {
            entry: "word/document.xml".into(),
            claimed_size: 200_000_000,
            limit: 100_000_000,
        };
        assert!(e.to_string().contains("ZIP bomb detected"));
        assert!(e.to_string().contains("word/document.xml"));
    }

    #[test]
    fn error_display_xml_parse() {
        let e = DocxError::XmlParse {
            part: "[Content_Types].xml".into(),
            reason: "unexpected EOF".into(),
        };
        assert!(e.to_string().contains("[Content_Types].xml"));
    }

    #[test]
    fn error_display_missing_part() {
        let e = DocxError::MissingPart("word/document.xml".into());
        assert_eq!(
            e.to_string(),
            "Missing required OOXML part: 'word/document.xml'"
        );
    }

    #[test]
    fn error_display_circular_style() {
        let e = DocxError::CircularStyleReference("Heading1".into());
        assert!(e.to_string().contains("circular reference"));
    }

    #[test]
    fn error_display_numbering_not_found() {
        let e = DocxError::NumberingDefNotFound { num_id: 42 };
        assert!(e.to_string().contains("42"));
    }

    #[test]
    fn error_display_entity_limit() {
        let e = DocxError::XmlEntityExpansionLimit {
            part: "word/document.xml".into(),
            limit: 100,
        };
        assert!(e.to_string().contains("100"));
    }

    #[test]
    fn error_display_path_traversal() {
        let e = DocxError::ZipPathTraversal {
            path: "../etc/passwd".into(),
        };
        assert!(e.to_string().contains("path traversal"));
        assert!(e.to_string().contains("../etc/passwd"));
    }

    #[test]
    fn result_alias_is_docx_error() {
        let r: Result<i32> = Err(DocxError::Pipeline("test".into()));
        assert!(r.is_err());
    }

    #[test]
    fn error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DocxError>();
    }

    #[test]
    fn error_from_io() {
        use std::io::{Error as IoError, ErrorKind};
        let io = IoError::new(ErrorKind::NotFound, "not found");
        let e = DocxError::from(io);
        assert!(matches!(e, DocxError::Io(_)));
    }

    #[test]
    fn error_from_zip() {
        let zip_err = zip::result::ZipError::FileNotFound;
        let e = DocxError::from(zip_err);
        assert!(matches!(e, DocxError::Zip(_)));
    }
}
