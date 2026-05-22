use std::io::Write;

use oxidize_docx::{DocxDocument, DocxElement};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>
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
