use crate::error::Result;
use crate::images::metadata::{detect_content_type, ImageMetadata};
use crate::zip::SecureZipArchive;

/// Walks the ZIP archive for entries under `word/media/`, extracts their
/// bytes, and sniffs each one's content type. Results are sorted by path
/// so the public `DocxDocument::images()` API is deterministic across
/// runs regardless of the underlying ZIP iteration order.
pub(crate) fn extract_images(archive: &mut SecureZipArchive) -> Result<Vec<ImageMetadata>> {
    let mut media_paths: Vec<String> = archive
        .entry_names()
        .into_iter()
        .filter(|name| name.starts_with("word/media/"))
        .collect();
    media_paths.sort();

    let mut images = Vec::with_capacity(media_paths.len());
    for path in media_paths {
        let bytes = archive.read_entry(&path)?;
        let content_type = detect_content_type(&bytes);
        images.push(ImageMetadata {
            path,
            bytes,
            content_type,
        });
    }
    Ok(images)
}
