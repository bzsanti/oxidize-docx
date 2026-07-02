use std::io::Write;

use oxidize_docx::{DocxDocument, DocxError};

/// Creates a minimal valid DOCX file at the given path.
///
/// Contains only the bare minimum for `DocxDocument::open()` to succeed:
/// - `[Content_Types].xml` with main document content type
/// - `_rels/.rels` with root relationship
/// - `word/document.xml` with empty body
fn make_minimal_docx(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    )
    .unwrap();

    zip.start_file("_rels/.rels", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
    )
    .unwrap();

    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body/>
</w:document>"#,
    )
    .unwrap();

    zip.finish().unwrap();
}

#[test]
fn open_minimal_docx_succeeds() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    make_minimal_docx(tmp.path());
    let result = DocxDocument::open(tmp.path());
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
}

#[test]
fn open_nonexistent_file_returns_io_error() {
    let result = DocxDocument::open("/nonexistent/path/file.docx");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DocxError::Io(_)));
}

#[test]
fn open_non_zip_file_returns_zip_error() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    tmp.write_all(b"this is not a zip file").unwrap();
    let result = DocxDocument::open(tmp.path());
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DocxError::Zip(_)));
}

#[test]
fn open_zip_without_content_types_returns_missing_part() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let file = std::fs::File::create(tmp.path()).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("dummy.txt", options).unwrap();
    zip.write_all(b"hello").unwrap();
    zip.finish().unwrap();

    let result = DocxDocument::open(tmp.path());
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DocxError::MissingPart(_)));
}

#[test]
fn open_zip_without_main_document_returns_missing_part() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let file = std::fs::File::create(tmp.path()).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    // Content types without main document override
    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/xml"/>
</Types>"#,
    )
    .unwrap();
    zip.finish().unwrap();

    let result = DocxDocument::open(tmp.path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, DocxError::MissingPart(ref msg) if msg.contains("main document")),
        "Expected MissingPart about main document, got: {err:?}"
    );
}
