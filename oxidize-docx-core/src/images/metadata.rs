/// Raw image bytes extracted from a DOCX's `word/media/` folder, along
/// with enough metadata for downstream consumers to identify and route
/// them without re-sniffing the bytes themselves.
///
/// `content_type` is derived from magic-byte sniffing — the OOXML
/// `[Content_Types].xml` manifest is consulted only as a fallback when
/// the magic header is ambiguous (today: never; the sniffer recognises
/// the common raster formats). This keeps the extractor self-contained
/// and lets it work on archives whose Content_Types lie or omit the
/// override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageMetadata {
    pub path: String,
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// Sniffs the leading bytes of an image and returns a MIME content type.
/// Falls back to `application/octet-stream` when the prefix doesn't
/// match any of the supported raster formats so the extractor never
/// silently drops an unrecognised image part.
pub(crate) fn detect_content_type(bytes: &[u8]) -> String {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png".to_string();
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return "image/jpeg".to_string();
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return "image/gif".to_string();
    }
    if bytes.starts_with(b"BM") {
        return "image/bmp".to_string();
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp".to_string();
    }
    "application/octet-stream".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_signature_is_recognised() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0DIHDR";
        assert_eq!(detect_content_type(png), "image/png");
    }

    #[test]
    fn jpeg_soi_marker_is_recognised() {
        let jpeg = b"\xff\xd8\xff\xe0\x00\x10JFIF";
        assert_eq!(detect_content_type(jpeg), "image/jpeg");
    }

    #[test]
    fn gif87a_and_gif89a_are_both_recognised() {
        assert_eq!(detect_content_type(b"GIF87a___"), "image/gif");
        assert_eq!(detect_content_type(b"GIF89a___"), "image/gif");
    }

    #[test]
    fn bmp_signature_is_recognised() {
        assert_eq!(detect_content_type(b"BM\x00\x00\x00\x00rest"), "image/bmp");
    }

    #[test]
    fn webp_recognised_via_riff_container() {
        assert_eq!(
            detect_content_type(b"RIFF\x24\x00\x00\x00WEBPVP8"),
            "image/webp"
        );
    }

    #[test]
    fn unknown_prefix_falls_back_to_octet_stream() {
        assert_eq!(
            detect_content_type(b"definitely not an image"),
            "application/octet-stream"
        );
    }

    #[test]
    fn empty_input_falls_back_to_octet_stream() {
        assert_eq!(detect_content_type(b""), "application/octet-stream");
    }
}
