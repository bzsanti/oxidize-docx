use crate::error::{DocxError, Result};

pub const MAX_ZIP_ENTRY_COUNT: usize = 10_000;
pub const MAX_UNCOMPRESSED_ENTRY_SIZE: u64 = 100 * 1024 * 1024; // 100 MB
pub const MAX_TOTAL_UNCOMPRESSED_SIZE: u64 = 500 * 1024 * 1024; // 500 MB
pub const MIN_COMPRESSION_RATIO: f64 = 0.001; // reject 1000:1+

/// Validates a single ZIP entry's size and compression ratio.
///
/// Rejects entries that exceed the maximum uncompressed size or have
/// a suspiciously low compression ratio (ZIP bomb indicator).
pub fn validate_entry_size(name: &str, uncompressed_size: u64, compressed_size: u64) -> Result<()> {
    if uncompressed_size > MAX_UNCOMPRESSED_ENTRY_SIZE {
        return Err(DocxError::ZipBomb {
            entry: name.to_string(),
            claimed_size: uncompressed_size,
            limit: MAX_UNCOMPRESSED_ENTRY_SIZE,
        });
    }

    if compressed_size > 0 && uncompressed_size > 0 {
        let ratio = compressed_size as f64 / uncompressed_size as f64;
        if ratio < MIN_COMPRESSION_RATIO {
            return Err(DocxError::ZipBomb {
                entry: name.to_string(),
                claimed_size: uncompressed_size,
                limit: MAX_UNCOMPRESSED_ENTRY_SIZE,
            });
        }
    }

    Ok(())
}

/// Validates the total number of entries in a ZIP archive.
pub fn validate_entry_count(count: usize) -> Result<()> {
    if count > MAX_ZIP_ENTRY_COUNT {
        return Err(DocxError::ZipEntryLimitExceeded {
            count,
            limit: MAX_ZIP_ENTRY_COUNT,
        });
    }
    Ok(())
}

/// Validates a ZIP entry path against path traversal attacks.
///
/// Rejects absolute paths and paths containing `..` components.
pub fn validate_zip_path(path: &str) -> Result<()> {
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(DocxError::ZipPathTraversal {
            path: path.to_string(),
        });
    }

    for component in path.split(['/', '\\']) {
        if component == ".." {
            return Err(DocxError::ZipPathTraversal {
                path: path.to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_constants_are_correct() {
        assert_eq!(MAX_ZIP_ENTRY_COUNT, 10_000);
        assert_eq!(MAX_UNCOMPRESSED_ENTRY_SIZE, 100 * 1024 * 1024);
        assert_eq!(MAX_TOTAL_UNCOMPRESSED_SIZE, 500 * 1024 * 1024);
    }

    #[test]
    fn compression_ratio_limit_is_correct() {
        assert!((MIN_COMPRESSION_RATIO - 0.001).abs() < f64::EPSILON);
    }

    #[test]
    fn validate_entry_size_accepts_normal_file() {
        let result = validate_entry_size("word/document.xml", 50 * 1024 * 1024, 1024 * 1024);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_entry_size_rejects_oversized() {
        let result = validate_entry_size("evil.xml", MAX_UNCOMPRESSED_ENTRY_SIZE + 1, 100);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DocxError::ZipBomb { .. }));
    }

    #[test]
    fn validate_entry_size_rejects_zip_bomb_ratio() {
        // 1 byte compressed, 90MB uncompressed — malicious ratio
        let result = validate_entry_size("evil.xml", 90 * 1024 * 1024, 1);
        assert!(result.is_err());
    }

    #[test]
    fn validate_entry_size_accepts_zero_compressed() {
        // Stored (no compression) — compressed == 0 should not trigger ratio check
        let result = validate_entry_size("stored.xml", 1024, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_entry_count_accepts_normal() {
        assert!(validate_entry_count(500).is_ok());
    }

    #[test]
    fn validate_entry_count_accepts_exactly_limit() {
        assert!(validate_entry_count(MAX_ZIP_ENTRY_COUNT).is_ok());
    }

    #[test]
    fn validate_entry_count_rejects_excess() {
        let result = validate_entry_count(MAX_ZIP_ENTRY_COUNT + 1);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DocxError::ZipEntryLimitExceeded { .. }
        ));
    }

    #[test]
    fn validate_path_accepts_normal() {
        assert!(validate_zip_path("word/document.xml").is_ok());
        assert!(validate_zip_path("[Content_Types].xml").is_ok());
        assert!(validate_zip_path("_rels/.rels").is_ok());
    }

    #[test]
    fn validate_path_rejects_traversal() {
        assert!(validate_zip_path("../etc/passwd").is_err());
        assert!(validate_zip_path("word/../../evil").is_err());
    }

    #[test]
    fn validate_path_rejects_absolute() {
        assert!(validate_zip_path("/etc/passwd").is_err());
    }

    #[test]
    fn validate_path_rejects_backslash_traversal() {
        assert!(validate_zip_path("word\\..\\..\\evil").is_err());
        assert!(validate_zip_path("\\etc\\passwd").is_err());
    }
}
