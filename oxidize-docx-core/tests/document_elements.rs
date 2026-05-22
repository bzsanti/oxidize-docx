use std::io::Write;

use oxidize_docx::{DocxDocument, DocxElement};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>
  <Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/>
  <Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/>
  <Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/>
</Types>"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

/// Wraps a `<w:body>` payload in a full `word/document.xml` envelope.
fn document_xml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
{body}
  </w:body>
</w:document>"#
    )
}

/// Writes a DOCX zip with the given document body and optional styles/numbering payloads.
fn write_docx(
    path: &std::path::Path,
    body_xml: &str,
    styles_xml: Option<&str>,
    numbering_xml: Option<&str>,
) {
    write_docx_full(path, body_xml, styles_xml, numbering_xml, None);
}

/// Like `write_docx` but also lets the caller embed `word/footnotes.xml`
/// and `word/endnotes.xml` payloads — Phase 5 fixtures use this when
/// exercising footnote/endnote-aware paths.
fn write_docx_full(
    path: &std::path::Path,
    body_xml: &str,
    styles_xml: Option<&str>,
    numbering_xml: Option<&str>,
    footnotes_xml: Option<&str>,
) {
    write_docx_with_notes(
        path,
        body_xml,
        styles_xml,
        numbering_xml,
        footnotes_xml,
        None,
    );
}

fn write_docx_with_notes(
    path: &std::path::Path,
    body_xml: &str,
    styles_xml: Option<&str>,
    numbering_xml: Option<&str>,
    footnotes_xml: Option<&str>,
    endnotes_xml: Option<&str>,
) {
    write_docx_with_all(
        path,
        body_xml,
        styles_xml,
        numbering_xml,
        footnotes_xml,
        endnotes_xml,
        None,
    );
}

fn write_docx_with_all(
    path: &std::path::Path,
    body_xml: &str,
    styles_xml: Option<&str>,
    numbering_xml: Option<&str>,
    footnotes_xml: Option<&str>,
    endnotes_xml: Option<&str>,
    comments_xml: Option<&str>,
) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    zip.start_file("[Content_Types].xml", options).unwrap();
    zip.write_all(CONTENT_TYPES.as_bytes()).unwrap();

    zip.start_file("_rels/.rels", options).unwrap();
    zip.write_all(RELS.as_bytes()).unwrap();

    zip.start_file("word/document.xml", options).unwrap();
    zip.write_all(document_xml(body_xml).as_bytes()).unwrap();

    if let Some(s) = styles_xml {
        zip.start_file("word/styles.xml", options).unwrap();
        zip.write_all(s.as_bytes()).unwrap();
    }

    if let Some(n) = numbering_xml {
        zip.start_file("word/numbering.xml", options).unwrap();
        zip.write_all(n.as_bytes()).unwrap();
    }

    if let Some(f) = footnotes_xml {
        zip.start_file("word/footnotes.xml", options).unwrap();
        zip.write_all(f.as_bytes()).unwrap();
    }

    if let Some(e) = endnotes_xml {
        zip.start_file("word/endnotes.xml", options).unwrap();
        zip.write_all(e.as_bytes()).unwrap();
    }

    if let Some(c) = comments_xml {
        zip.start_file("word/comments.xml", options).unwrap();
        zip.write_all(c.as_bytes()).unwrap();
    }

    zip.finish().unwrap();
}

const STYLES_HEADING1: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
  </w:style>
</w:styles>"#;

const NUMBERING_DECIMAL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:start w:val="1"/>
      <w:numFmt w:val="decimal"/>
      <w:lvlText w:val="%1."/>
    </w:lvl>
  </w:abstractNum>
  <w:num w:numId="1">
    <w:abstractNumId w:val="0"/>
  </w:num>
</w:numbering>"#;

#[test]
fn elements_resolves_two_decimal_list_items_with_indices_1_and_2() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body = r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>first</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>second</w:t></w:r></w:p>"#;
    write_docx(tmp.path(), body, None, Some(NUMBERING_DECIMAL));

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let elements = doc.elements().expect("elements");

    use oxidize_docx::pipeline::ListType;
    assert_eq!(
        elements,
        vec![
            DocxElement::ListItem {
                text: "first".into(),
                level: 0,
                list_type: ListType::Decimal,
                display_index: Some(1),
            },
            DocxElement::ListItem {
                text: "second".into(),
                level: 0,
                list_type: ListType::Decimal,
                display_index: Some(2),
            },
        ]
    );
}

#[test]
fn elements_resolves_2x2_table_with_grid_span_and_vmerge() {
    use oxidize_docx::pipeline::{TableCell, TableRow};

    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    // Row 1: cell A (1x1), cell B (gridSpan=2, vMerge=restart spans col 1-2 down).
    // Row 2: cell C (1x1), cell D (gridSpan=2, vMerge=continue → absorbed by B).
    let body = r#"<w:tbl>
  <w:tr>
    <w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
    <w:tc>
      <w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr>
      <w:p><w:r><w:t>B</w:t></w:r></w:p>
    </w:tc>
  </w:tr>
  <w:tr>
    <w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc>
    <w:tc>
      <w:tcPr><w:gridSpan w:val="2"/><w:vMerge/></w:tcPr>
      <w:p/>
    </w:tc>
  </w:tr>
</w:tbl>"#;
    write_docx(tmp.path(), body, None, None);

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let elements = doc.elements().expect("elements");

    assert_eq!(
        elements,
        vec![DocxElement::Table {
            rows: vec![
                TableRow {
                    cells: vec![
                        TableCell {
                            text: "A".into(),
                            col_span: 1,
                            row_span: 1,
                        },
                        TableCell {
                            text: "B".into(),
                            col_span: 2,
                            row_span: 2,
                        },
                    ],
                },
                TableRow {
                    cells: vec![TableCell {
                        text: "C".into(),
                        col_span: 1,
                        row_span: 1,
                    }],
                },
            ],
        }]
    );
}

#[test]
fn elements_classifies_pstyle_heading1_as_heading_level_1() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body = r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Intro</w:t></w:r></w:p>
<w:p><w:r><w:t>body</w:t></w:r></w:p>"#;
    write_docx(tmp.path(), body, Some(STYLES_HEADING1), None);

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let elements = doc.elements().expect("elements");

    use oxidize_docx::pipeline::HeadingContext;
    assert_eq!(
        elements,
        vec![
            DocxElement::Heading {
                level: 1,
                text: "Intro".into(),
            },
            DocxElement::Paragraph {
                text: "body".into(),
                parent_heading: Some(HeadingContext {
                    level: 1,
                    text: "Intro".into(),
                }),
            },
        ]
    );
}

const FOOTNOTES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:footnote w:id="-1" w:type="separator"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
  <w:footnote w:id="0" w:type="continuationSeparator"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>
  <w:footnote w:id="1"><w:p><w:r><w:t>real footnote text</w:t></w:r></w:p></w:footnote>
</w:footnotes>"#;

const ENDNOTES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:endnote w:id="1"><w:p><w:r><w:t>endnote text</w:t></w:r></w:p></w:endnote>
</w:endnotes>"#;

const COMMENTS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:comment w:id="7" w:author="Alice"><w:p><w:r><w:t>needs revision</w:t></w:r></w:p></w:comment>
</w:comments>"#;

#[test]
fn elements_resolves_comment_reference_emitting_comment_with_author_and_text() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body = r#"<w:p><w:r><w:t>text</w:t></w:r><w:r><w:commentReference w:id="7"/></w:r></w:p>"#;
    write_docx_with_all(tmp.path(), body, None, None, None, None, Some(COMMENTS_XML));

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let elements = doc.elements().expect("elements");

    assert_eq!(
        elements,
        vec![
            DocxElement::Paragraph {
                text: "text".into(),
                parent_heading: None,
            },
            DocxElement::Comment {
                id: 7,
                author: "Alice".into(),
                text: "needs revision".into(),
            },
        ]
    );
}

#[test]
fn elements_resolves_endnote_reference_emitting_endnote_after_paragraph() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body =
        r#"<w:p><w:r><w:t>citation</w:t></w:r><w:r><w:endnoteReference w:id="1"/></w:r></w:p>"#;
    write_docx_with_notes(tmp.path(), body, None, None, None, Some(ENDNOTES_XML));

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let elements = doc.elements().expect("elements");

    assert_eq!(
        elements,
        vec![
            DocxElement::Paragraph {
                text: "citation".into(),
                parent_heading: None,
            },
            DocxElement::Endnote {
                id: 1,
                text: "endnote text".into(),
            },
        ]
    );
}

#[test]
fn elements_resolves_footnote_reference_to_docx_element_footnote() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body = r#"<w:p><w:r><w:t>See</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#;
    write_docx_full(tmp.path(), body, None, None, Some(FOOTNOTES_XML));

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let elements = doc.elements().expect("elements");

    assert_eq!(
        elements,
        vec![
            DocxElement::Paragraph {
                text: "See".into(),
                parent_heading: None,
            },
            DocxElement::Footnote {
                id: 1,
                text: "real footnote text".into(),
            },
        ]
    );
}

#[test]
fn rag_chunks_emits_per_heading_block_with_populated_context() {
    use oxidize_docx::pipeline::HeadingContext;

    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body = r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Intro</w:t></w:r></w:p>
<w:p><w:r><w:t>body</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Conclusion</w:t></w:r></w:p>
<w:p><w:r><w:t>end</w:t></w:r></w:p>"#;
    write_docx(tmp.path(), body, Some(STYLES_HEADING1), None);

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let chunks = doc.rag_chunks().expect("rag_chunks");

    assert_eq!(chunks.len(), 2);

    assert_eq!(chunks[0].text, "Intro\n\nbody");
    assert_eq!(chunks[0].paragraph_indices, vec![0, 1]);
    assert_eq!(
        chunks[0].element_types,
        vec!["heading".to_string(), "paragraph".to_string()]
    );
    assert_eq!(
        chunks[0].heading_context,
        vec![HeadingContext {
            level: 1,
            text: "Intro".into()
        }]
    );
    assert!(!chunks[0].is_oversized);

    assert_eq!(chunks[1].text, "Conclusion\n\nend");
    assert_eq!(chunks[1].paragraph_indices, vec![2, 3]);
    assert_eq!(
        chunks[1].heading_context,
        vec![HeadingContext {
            level: 1,
            text: "Conclusion".into()
        }]
    );
}

#[test]
fn to_markdown_renders_heading_list_paragraph_with_gfm_syntax() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body = r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>one</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>two</w:t></w:r></w:p>
<w:p><w:r><w:t>tail</w:t></w:r></w:p>"#;
    write_docx(
        tmp.path(),
        body,
        Some(STYLES_HEADING1),
        Some(NUMBERING_DECIMAL),
    );

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let md = doc.to_markdown().expect("to_markdown");

    assert_eq!(md, "# Title\n\n1. one\n2. two\n\ntail");
}

#[test]
fn plain_text_renders_heading_list_and_paragraph_with_expected_separators() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body = r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>one</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>two</w:t></w:r></w:p>
<w:p><w:r><w:t>tail</w:t></w:r></w:p>"#;
    write_docx(
        tmp.path(),
        body,
        Some(STYLES_HEADING1),
        Some(NUMBERING_DECIMAL),
    );

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let text = doc.plain_text().expect("plain_text");

    assert_eq!(text, "Title\n\none\ntwo\n\ntail");
}

#[test]
fn elements_returns_single_paragraph_for_minimal_docx() {
    let tmp = tempfile::NamedTempFile::with_suffix(".docx").unwrap();
    let body = r#"<w:p><w:r><w:t>Hello</w:t></w:r></w:p>"#;
    write_docx(tmp.path(), body, None, None);

    let doc = DocxDocument::open(tmp.path()).expect("open");
    let elements = doc.elements().expect("elements");

    assert_eq!(
        elements,
        vec![DocxElement::Paragraph {
            text: "Hello".into(),
            parent_heading: None,
        }]
    );
}
