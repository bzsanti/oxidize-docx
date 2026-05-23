use std::io::Write;

use oxidize_docx::DocxDocument;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="png" ContentType="image/png"/>
  <Default Extension="jpeg" ContentType="image/jpeg"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>doc</w:t></w:r></w:p></w:body>
</w:document>"#;

/// Minimal valid-looking PNG: the 8-byte signature followed by junk
/// payload bytes. The extractor only sniffs the magic header so the
/// rest can be arbitrary.
const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0DIHDR-fake-tail";

/// Minimal JPEG SOI marker + APP0 segment header + filler.
const JPEG_BYTES: &[u8] = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x02fake";

/// Writes a DOCX zip with the given media entries.
/// Each `(zip_path, bytes)` is stored verbatim as a deflated entry.
fn write_docx_with_media(path: &std::path::Path, media: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(CONTENT_TYPES.as_bytes()).unwrap();
    zip.start_file("_rels/.rels", options).unwrap();
    zip.write_all(RELS.as_bytes()).unwrap();
    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(DOCUMENT_XML.as_bytes()).unwrap();

    for (entry_path, bytes) in media {
        zip.start_file(*entry_path, options).unwrap();
        zip.write_all(bytes).unwrap();
    }

    zip.finish().unwrap();
}

#[test]
fn images_returns_empty_vec_when_word_media_is_absent() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    write_docx_with_media(tmp.path(), &[]);

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let images = doc.images().expect("images");

    assert!(images.is_empty());
}

#[test]
fn images_extracts_a_single_png_with_correct_content_type_and_bytes() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    write_docx_with_media(tmp.path(), &[("word/media/image1.png", PNG_BYTES)]);

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let images = doc.images().expect("images");

    assert_eq!(images.len(), 1);
    let img = &images[0];
    assert_eq!(img.path, "word/media/image1.png");
    assert_eq!(img.content_type, "image/png");
    assert_eq!(img.bytes.as_slice(), PNG_BYTES);
}

#[test]
fn images_returns_multiple_entries_sorted_by_path_for_deterministic_order() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    // Insert in reverse alphabetical order to confirm sort is applied.
    write_docx_with_media(
        tmp.path(),
        &[
            ("word/media/image2.jpeg", JPEG_BYTES),
            ("word/media/image1.png", PNG_BYTES),
        ],
    );

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let images = doc.images().expect("images");

    assert_eq!(images.len(), 2);
    assert_eq!(images[0].path, "word/media/image1.png");
    assert_eq!(images[0].content_type, "image/png");
    assert_eq!(images[1].path, "word/media/image2.jpeg");
    assert_eq!(images[1].content_type, "image/jpeg");
}
