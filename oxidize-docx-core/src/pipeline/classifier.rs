use std::collections::HashMap;

use crate::error::Result;
use crate::numbering::{NumberingDefs, NumberingResolver};
use crate::ooxml::relationships::RelationshipMap;
use crate::pipeline::element::{DocxElement, HeaderKind, HeadingContext, LinkSpan};
use crate::pipeline::table_builder::build_table;
use crate::raw::body::{RawBody, RawBodyItem, RawSectionRef};
use crate::raw::paragraphs::{RawHyperlink, RawInline, RawParagraph};
use crate::styles::resolver::StyleResolver;
use crate::styles::table::StyleTable;
use crate::word::comments_xml::CommentMap;
use crate::word::endnotes_xml::EndnoteMap;
use crate::word::footnotes_xml::FootnoteMap;

/// Walks a `RawBody` in document order and emits a `Vec<DocxElement>`.
/// Stateful across calls within the same instance — heading context and
/// list counters carry from one paragraph to the next.
#[allow(dead_code)]
pub(crate) struct ClassifierPipeline<'a> {
    style_table: &'a StyleTable,
    numbering_defs: &'a NumberingDefs,
    numbering_resolver: NumberingResolver<'a>,
    footnotes: Option<&'a FootnoteMap>,
    endnotes: Option<&'a EndnoteMap>,
    comments: Option<&'a CommentMap>,
    relationships: Option<&'a RelationshipMap>,
    header_bodies: Option<&'a HashMap<String, RawBody>>,
    footer_bodies: Option<&'a HashMap<String, RawBody>>,
    current_heading: Option<HeadingContext>,
}

#[allow(dead_code)]
impl<'a> ClassifierPipeline<'a> {
    pub(crate) fn new(style_table: &'a StyleTable, numbering_defs: &'a NumberingDefs) -> Self {
        Self {
            style_table,
            numbering_defs,
            numbering_resolver: NumberingResolver::new(numbering_defs),
            footnotes: None,
            endnotes: None,
            comments: None,
            relationships: None,
            header_bodies: None,
            footer_bodies: None,
            current_heading: None,
        }
    }

    pub(crate) fn with_footnotes(mut self, footnotes: &'a FootnoteMap) -> Self {
        self.footnotes = Some(footnotes);
        self
    }

    pub(crate) fn with_endnotes(mut self, endnotes: &'a EndnoteMap) -> Self {
        self.endnotes = Some(endnotes);
        self
    }

    pub(crate) fn with_comments(mut self, comments: &'a CommentMap) -> Self {
        self.comments = Some(comments);
        self
    }

    pub(crate) fn with_relationships(mut self, relationships: &'a RelationshipMap) -> Self {
        self.relationships = Some(relationships);
        self
    }

    /// Provides the classifier with the parsed `<w:hdr>`/`<w:ftr>` bodies
    /// (keyed by the archive path the relationship target resolves to,
    /// e.g. `"word/header1.xml"`). When a `<w:sectPr>` is reached the
    /// classifier looks each header/footer ref up and classifies the
    /// matching body recursively into a `DocxElement::Header` /
    /// `DocxElement::Footer`.
    pub(crate) fn with_section_bodies(
        mut self,
        header_bodies: &'a HashMap<String, RawBody>,
        footer_bodies: &'a HashMap<String, RawBody>,
    ) -> Self {
        self.header_bodies = Some(header_bodies);
        self.footer_bodies = Some(footer_bodies);
        self
    }

    pub(crate) fn classify(&mut self, body: &RawBody) -> Result<Vec<DocxElement>> {
        let mut out = Vec::with_capacity(body.items.len());
        for item in &body.items {
            if let RawBodyItem::Table(t) = item {
                out.push(DocxElement::Table {
                    rows: build_table(t),
                });
                continue;
            }
            if let RawBodyItem::SectionBreak(props) = item {
                for href in &props.header_refs {
                    if let Some(elem) = self.build_section_element(href, SectionPart::Header)? {
                        out.push(elem);
                    }
                }
                for fref in &props.footer_refs {
                    if let Some(elem) = self.build_section_element(fref, SectionPart::Footer)? {
                        out.push(elem);
                    }
                }
                continue;
            }
            if let RawBodyItem::Paragraph(p) = item {
                let text = paragraph_text(p);
                let footnote_refs = p.footnote_ref_ids.clone();
                let endnote_refs = p.endnote_ref_ids.clone();
                let comment_refs = p.comment_ref_ids.clone();
                let links = self.collect_link_spans(p);
                let element = if let Some(num_pr) = &p.properties.num_pr {
                    let info = self
                        .numbering_resolver
                        .advance(num_pr.num_id, num_pr.ilvl)?;
                    DocxElement::ListItem {
                        text,
                        level: info.ilvl,
                        list_type: info.list_type,
                        display_index: info.display_index,
                    }
                } else {
                    match self.heading_level(p)? {
                        Some(level) => {
                            self.current_heading = Some(HeadingContext {
                                level,
                                text: text.clone(),
                            });
                            DocxElement::Heading { level, text }
                        }
                        None => DocxElement::Paragraph {
                            text,
                            parent_heading: self.current_heading.clone(),
                            links,
                        },
                    }
                };
                out.push(element);
                if let Some(footnotes) = self.footnotes {
                    for id in &footnote_refs {
                        if let Some(text) = footnotes.get(*id) {
                            out.push(DocxElement::Footnote {
                                id: *id,
                                text: text.to_string(),
                            });
                        }
                    }
                }
                if let Some(endnotes) = self.endnotes {
                    for id in &endnote_refs {
                        if let Some(text) = endnotes.get(*id) {
                            out.push(DocxElement::Endnote {
                                id: *id,
                                text: text.to_string(),
                            });
                        }
                    }
                }
                if let Some(comments) = self.comments {
                    for id in &comment_refs {
                        if let Some(info) = comments.get(*id) {
                            out.push(DocxElement::Comment {
                                id: *id,
                                author: info.author.clone(),
                                text: info.text.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Walks the paragraph's inline content and produces a `LinkSpan` for
    /// every hyperlink that resolves to a non-empty URL. The spans appear
    /// in the same order they do in the source text, so an exporter that
    /// matches them against `paragraph.text` finds them in left-to-right
    /// order.
    fn collect_link_spans(&self, p: &RawParagraph) -> Vec<LinkSpan> {
        let mut spans = Vec::new();
        for inline in &p.content {
            if let RawInline::Hyperlink(link) = inline {
                if let Some(url) = resolve_hyperlink_url(link, self.relationships) {
                    spans.push(LinkSpan {
                        text: hyperlink_text(link),
                        url,
                    });
                }
            }
        }
        spans
    }

    /// Resolves a `<w:headerReference>`/`<w:footerReference>` into a
    /// fully-classified `DocxElement::Header`/`Footer`, or returns
    /// `Ok(None)` if any link in the chain is missing (no relationships,
    /// unknown rel_id, no body provided for the target path).
    ///
    /// A fresh `ClassifierPipeline` is built per section to give headers
    /// and footers their own numbering counters and heading context —
    /// sharing state with the main body would let an "Heading 1" inside
    /// a header pollute the main flow.
    fn build_section_element(
        &self,
        sref: &RawSectionRef,
        part: SectionPart,
    ) -> Result<Option<DocxElement>> {
        let rels = match self.relationships {
            Some(r) => r,
            None => return Ok(None),
        };
        let Some(rel) = rels.get_by_id(&sref.rel_id) else {
            return Ok(None);
        };
        let path = format!("word/{}", rel.target);

        let bodies = match part {
            SectionPart::Header => self.header_bodies,
            SectionPart::Footer => self.footer_bodies,
        };
        let Some(map) = bodies else {
            return Ok(None);
        };
        let Some(part_body) = map.get(&path) else {
            return Ok(None);
        };

        let mut inner = ClassifierPipeline::new(self.style_table, self.numbering_defs);
        if let Some(r) = self.relationships {
            inner = inner.with_relationships(r);
        }
        if let Some(fn_) = self.footnotes {
            inner = inner.with_footnotes(fn_);
        }
        if let Some(en_) = self.endnotes {
            inner = inner.with_endnotes(en_);
        }
        if let Some(c_) = self.comments {
            inner = inner.with_comments(c_);
        }
        let content = inner.classify(part_body)?;
        let kind = HeaderKind::from(&sref.ref_type);
        Ok(Some(match part {
            SectionPart::Header => DocxElement::Header { kind, content },
            SectionPart::Footer => DocxElement::Footer { kind, content },
        }))
    }

    /// Returns 1..=9 when the paragraph is a heading.
    ///
    /// Resolution order:
    ///   1. Ask `StyleResolver::resolve_paragraph` — uses outlineLvl
    ///      from the merged 4-layer pPr chain (canonical OOXML signal).
    ///   2. Fallback to the legacy "heading N" style-name heuristic for
    ///      documents whose styles never declare outlineLvl explicitly
    ///      but follow Word's naming convention.
    ///
    /// Returns `Err` only if the style chain is malformed (cycle or
    /// depth > MAX_STYLE_DEPTH).
    fn heading_level(&self, p: &RawParagraph) -> Result<Option<u8>> {
        let resolver = StyleResolver::new(self.style_table);
        if let Some(level) = resolver.resolve_paragraph(p)?.heading_level {
            return Ok(Some(level));
        }
        Ok(self.heading_level_by_name(p))
    }

    fn heading_level_by_name(&self, p: &RawParagraph) -> Option<u8> {
        let style_id = p.properties.style_id.as_deref()?;
        let style = self.style_table.get(style_id)?;
        let name = style.name.to_ascii_lowercase();
        let rest = name.strip_prefix("heading ")?;
        let digit = rest.chars().next()?;
        if digit.is_ascii_digit() && digit != '0' {
            Some(digit.to_digit(10)? as u8)
        } else {
            None
        }
    }
}

#[derive(Copy, Clone)]
enum SectionPart {
    Header,
    Footer,
}

fn paragraph_text(p: &RawParagraph) -> String {
    let mut s = String::new();
    for inline in &p.content {
        match inline {
            RawInline::Run(run) => {
                if let Some(t) = &run.text {
                    s.push_str(t);
                }
            }
            RawInline::Hyperlink(link) => {
                for r in &link.runs {
                    if let Some(t) = &r.text {
                        s.push_str(t);
                    }
                }
            }
        }
    }
    s
}

/// Resolves a raw hyperlink to its display URL. Order of preference:
///   1. `rel_id` resolves to an entry in `relationships` → use its `target`.
///   2. `anchor` is present → use `#anchor` (in-document reference).
///   3. Otherwise → `None` (the link is dropped from `paragraph.links`).
///
/// An empty resolved URL also returns `None` so the paragraph doesn't end
/// up with a "link to nowhere".
fn resolve_hyperlink_url(
    link: &RawHyperlink,
    relationships: Option<&RelationshipMap>,
) -> Option<String> {
    if let (Some(rel_id), Some(rels)) = (link.rel_id.as_deref(), relationships) {
        if let Some(rel) = rels.get_by_id(rel_id) {
            if !rel.target.is_empty() {
                return Some(rel.target.clone());
            }
        }
    }
    link.anchor.as_deref().map(|a| format!("#{a}"))
}

fn hyperlink_text(link: &RawHyperlink) -> String {
    let mut s = String::new();
    for run in &link.runs {
        if let Some(t) = &run.text {
            s.push_str(t);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::element::{TableCell, TableRow};
    use crate::raw::body::RawBodyItem;
    use crate::raw::paragraphs::{RawParagraph, RawParagraphProperties};
    use crate::raw::runs::{RawRun, RawRunProperties};
    use crate::styles::table::{StyleEntry, StyleType};

    fn paragraph_style(id: &str, name: &str) -> StyleEntry {
        StyleEntry {
            style_id: id.into(),
            name: name.into(),
            style_type: StyleType::Paragraph,
            based_on: None,
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: None,
        }
    }

    fn paragraph_with(style_id: Option<&str>, runs: Vec<RawRun>) -> RawParagraph {
        RawParagraph {
            properties: RawParagraphProperties {
                style_id: style_id.map(|s| s.into()),
                ..Default::default()
            },
            content: runs.into_iter().map(RawInline::Run).collect(),
            ..Default::default()
        }
    }

    fn run(text: &str) -> RawRun {
        RawRun {
            text: Some(text.into()),
            properties: RawRunProperties::default(),
        }
    }

    fn paragraph_with_runs(runs: Vec<RawRun>) -> RawParagraph {
        RawParagraph {
            content: runs.into_iter().map(RawInline::Run).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn vmerge_restart_then_continue_yields_row_span_2_and_drops_continuation() {
        use crate::raw::tables::{
            RawTable, RawTableCell, RawTableCellProperties, RawTableProperties, RawTableRow,
            RawVMerge,
        };

        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let restart_cell = RawTableCell {
            properties: RawTableCellProperties {
                v_merge: Some(RawVMerge::Restart),
                ..Default::default()
            },
            paragraphs: vec![paragraph_with(None, vec![run("top")])],
        };
        let continue_cell = RawTableCell {
            properties: RawTableCellProperties {
                v_merge: Some(RawVMerge::Continue),
                ..Default::default()
            },
            paragraphs: vec![],
        };

        let table = RawTable {
            properties: RawTableProperties::default(),
            rows: vec![
                RawTableRow {
                    cells: vec![restart_cell],
                },
                RawTableRow {
                    cells: vec![continue_cell],
                },
            ],
        };
        let body = RawBody {
            items: vec![RawBodyItem::Table(table)],
        };

        let elements = classifier.classify(&body).unwrap();
        assert_eq!(
            elements,
            vec![DocxElement::Table {
                rows: vec![
                    TableRow {
                        cells: vec![TableCell {
                            text: "top".into(),
                            col_span: 1,
                            row_span: 2,
                        }],
                    },
                    TableRow { cells: vec![] },
                ],
            }]
        );
    }

    #[test]
    fn cell_grid_span_3_collapses_into_single_table_cell_with_col_span_3() {
        use crate::raw::tables::{
            RawTable, RawTableCell, RawTableCellProperties, RawTableProperties, RawTableRow,
        };

        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let table = RawTable {
            properties: RawTableProperties::default(),
            rows: vec![RawTableRow {
                cells: vec![RawTableCell {
                    properties: RawTableCellProperties {
                        grid_span: 3,
                        ..Default::default()
                    },
                    paragraphs: vec![paragraph_with(None, vec![run("wide")])],
                }],
            }],
        };

        let body = RawBody {
            items: vec![RawBodyItem::Table(table)],
        };

        let elements = classifier.classify(&body).unwrap();
        assert_eq!(
            elements,
            vec![DocxElement::Table {
                rows: vec![TableRow {
                    cells: vec![TableCell {
                        text: "wide".into(),
                        col_span: 3,
                        row_span: 1,
                    }],
                }],
            }]
        );
    }

    #[test]
    fn table_with_one_cell_becomes_table_element_with_single_row_and_cell() {
        use crate::raw::tables::{
            RawTable, RawTableCell, RawTableCellProperties, RawTableProperties, RawTableRow,
        };

        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let table = RawTable {
            properties: RawTableProperties::default(),
            rows: vec![RawTableRow {
                cells: vec![RawTableCell {
                    properties: RawTableCellProperties::default(),
                    paragraphs: vec![paragraph_with(None, vec![run("A")])],
                }],
            }],
        };

        let body = RawBody {
            items: vec![RawBodyItem::Table(table)],
        };

        let elements = classifier.classify(&body).unwrap();
        assert_eq!(
            elements,
            vec![DocxElement::Table {
                rows: vec![TableRow {
                    cells: vec![TableCell {
                        text: "A".into(),
                        col_span: 1,
                        row_span: 1,
                    }],
                }],
            }]
        );
    }

    #[test]
    fn paragraph_with_comment_ref_emits_comment_after_endnotes() {
        use crate::word::comments_xml::{CommentInfo, CommentMap};

        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let mut comments = CommentMap::new();
        comments.insert(
            0,
            CommentInfo {
                author: "Reviewer".into(),
                text: "needs work".into(),
            },
        );
        let mut classifier = ClassifierPipeline::new(&styles, &numbering).with_comments(&comments);

        let mut p = paragraph_with(None, vec![run("body")]);
        p.comment_ref_ids = vec![0];

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(p)],
        };
        let elements = classifier.classify(&body).unwrap();

        assert_eq!(
            elements,
            vec![
                DocxElement::Paragraph {
                    text: "body".into(),
                    parent_heading: None,
                    links: vec![],
                },
                DocxElement::Comment {
                    id: 0,
                    author: "Reviewer".into(),
                    text: "needs work".into(),
                },
            ]
        );
    }

    #[test]
    fn paragraph_with_endnote_ref_emits_endnote_after_footnotes() {
        use crate::word::endnotes_xml::EndnoteMap;
        use crate::word::footnotes_xml::FootnoteMap;

        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let mut footnotes = FootnoteMap::new();
        footnotes.insert(1, "fn1".into());
        let mut endnotes = EndnoteMap::new();
        endnotes.insert(2, "en2".into());
        let mut classifier = ClassifierPipeline::new(&styles, &numbering)
            .with_footnotes(&footnotes)
            .with_endnotes(&endnotes);

        let mut p = paragraph_with(None, vec![run("body")]);
        p.footnote_ref_ids = vec![1];
        p.endnote_ref_ids = vec![2];

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(p)],
        };
        let elements = classifier.classify(&body).unwrap();

        // Order: paragraph → footnotes → endnotes
        assert_eq!(
            elements,
            vec![
                DocxElement::Paragraph {
                    text: "body".into(),
                    parent_heading: None,
                    links: vec![],
                },
                DocxElement::Footnote {
                    id: 1,
                    text: "fn1".into(),
                },
                DocxElement::Endnote {
                    id: 2,
                    text: "en2".into(),
                },
            ]
        );
    }

    #[test]
    fn paragraph_with_footnote_ref_emits_footnote_element_after_paragraph() {
        use crate::word::footnotes_xml::FootnoteMap;

        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let mut footnotes = FootnoteMap::new();
        footnotes.insert(1, "explanation".into());
        let mut classifier =
            ClassifierPipeline::new(&styles, &numbering).with_footnotes(&footnotes);

        let mut p = paragraph_with(None, vec![run("body")]);
        p.footnote_ref_ids = vec![1];

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(p)],
        };
        let elements = classifier.classify(&body).unwrap();

        assert_eq!(
            elements,
            vec![
                DocxElement::Paragraph {
                    text: "body".into(),
                    parent_heading: None,
                    links: vec![],
                },
                DocxElement::Footnote {
                    id: 1,
                    text: "explanation".into(),
                },
            ]
        );
    }

    #[test]
    fn document_order_preserved_with_heading_paragraph_list_item_paragraph() {
        use crate::numbering::defs::{AbstractNum, ConcreteNum, NumberingLevel};
        use crate::numbering::ListType;
        use crate::raw::paragraphs::RawNumPr;

        let mut styles = StyleTable::new();
        styles.insert(paragraph_style("Heading1", "heading 1"));

        let mut numbering = NumberingDefs::new();
        numbering.insert_abstract(AbstractNum {
            abstract_num_id: 0,
            levels: vec![NumberingLevel {
                ilvl: 0,
                start: 1,
                num_fmt: "decimal".into(),
                level_text: "%1.".into(),
                ..Default::default()
            }],
        });
        numbering.insert_concrete(ConcreteNum {
            num_id: 1,
            abstract_num_id: 0,
            level_overrides: vec![],
        });

        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let mut li = paragraph_with(None, vec![run("li-text")]);
        li.properties.num_pr = Some(RawNumPr { num_id: 1, ilvl: 0 });

        let body = RawBody {
            items: vec![
                RawBodyItem::Paragraph(paragraph_with(Some("Heading1"), vec![run("A")])),
                RawBodyItem::Paragraph(paragraph_with(None, vec![run("p1")])),
                RawBodyItem::Paragraph(li),
                RawBodyItem::Paragraph(paragraph_with(None, vec![run("p2")])),
            ],
        };

        let elements = classifier.classify(&body).unwrap();
        let parent_a = Some(HeadingContext {
            level: 1,
            text: "A".into(),
        });

        assert_eq!(
            elements,
            vec![
                DocxElement::Heading {
                    level: 1,
                    text: "A".into()
                },
                DocxElement::Paragraph {
                    text: "p1".into(),
                    parent_heading: parent_a.clone(),
                    links: vec![],
                },
                DocxElement::ListItem {
                    text: "li-text".into(),
                    level: 0,
                    list_type: ListType::Decimal,
                    display_index: Some(1),
                },
                DocxElement::Paragraph {
                    text: "p2".into(),
                    parent_heading: parent_a,
                    links: vec![],
                },
            ]
        );
    }

    #[test]
    fn paragraph_with_num_pr_becomes_list_item_with_decimal_index() {
        use crate::numbering::defs::{AbstractNum, ConcreteNum, NumberingLevel};
        use crate::numbering::ListType;
        use crate::raw::paragraphs::RawNumPr;

        let styles = StyleTable::new();
        let mut numbering = NumberingDefs::new();
        numbering.insert_abstract(AbstractNum {
            abstract_num_id: 0,
            levels: vec![NumberingLevel {
                ilvl: 0,
                start: 1,
                num_fmt: "decimal".into(),
                level_text: "%1.".into(),
                ..Default::default()
            }],
        });
        numbering.insert_concrete(ConcreteNum {
            num_id: 1,
            abstract_num_id: 0,
            level_overrides: vec![],
        });
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let mut p = paragraph_with(None, vec![run("item one")]);
        p.properties.num_pr = Some(RawNumPr { num_id: 1, ilvl: 0 });

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(p)],
        };
        let elements = classifier.classify(&body).unwrap();

        assert_eq!(
            elements,
            vec![DocxElement::ListItem {
                text: "item one".into(),
                level: 0,
                list_type: ListType::Decimal,
                display_index: Some(1),
            }]
        );
    }

    #[test]
    fn paragraph_after_heading_carries_parent_heading_context() {
        let mut styles = StyleTable::new();
        styles.insert(paragraph_style("Heading1", "heading 1"));
        let numbering = NumberingDefs::new();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let body = RawBody {
            items: vec![
                RawBodyItem::Paragraph(paragraph_with(Some("Heading1"), vec![run("Intro")])),
                RawBodyItem::Paragraph(paragraph_with(None, vec![run("body text")])),
            ],
        };

        let elements = classifier.classify(&body).unwrap();
        assert_eq!(
            elements,
            vec![
                DocxElement::Heading {
                    level: 1,
                    text: "Intro".into()
                },
                DocxElement::Paragraph {
                    text: "body text".into(),
                    parent_heading: Some(HeadingContext {
                        level: 1,
                        text: "Intro".into()
                    }),
                    links: vec![],
                },
            ]
        );
    }

    #[test]
    fn classifier_emits_header_and_footer_for_resolved_section_refs() {
        use crate::ooxml::relationships::RelationshipMap;
        use crate::pipeline::element::HeaderKind;
        use crate::raw::body::{RawSectionProperties, RawSectionRef, SectionRefType};
        use std::collections::HashMap;

        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let rels_xml = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId10" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>
  <Relationship Id="rId20" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/>
</Relationships>"#;
        let rels = RelationshipMap::parse(rels_xml, "word/_rels/document.xml.rels").unwrap();

        let mut header_bodies: HashMap<String, RawBody> = HashMap::new();
        header_bodies.insert(
            "word/header1.xml".into(),
            RawBody {
                items: vec![RawBodyItem::Paragraph(paragraph_with(
                    None,
                    vec![run("page header")],
                ))],
            },
        );
        let mut footer_bodies: HashMap<String, RawBody> = HashMap::new();
        footer_bodies.insert(
            "word/footer1.xml".into(),
            RawBody {
                items: vec![RawBodyItem::Paragraph(paragraph_with(
                    None,
                    vec![run("page footer")],
                ))],
            },
        );

        let mut classifier = ClassifierPipeline::new(&styles, &numbering)
            .with_relationships(&rels)
            .with_section_bodies(&header_bodies, &footer_bodies);

        let body = RawBody {
            items: vec![
                RawBodyItem::Paragraph(paragraph_with(None, vec![run("body")])),
                RawBodyItem::SectionBreak(RawSectionProperties {
                    header_refs: vec![RawSectionRef {
                        rel_id: "rId10".into(),
                        ref_type: SectionRefType::Default,
                    }],
                    footer_refs: vec![RawSectionRef {
                        rel_id: "rId20".into(),
                        ref_type: SectionRefType::Default,
                    }],
                }),
            ],
        };

        let elements = classifier.classify(&body).unwrap();
        assert_eq!(
            elements,
            vec![
                DocxElement::Paragraph {
                    text: "body".into(),
                    parent_heading: None,
                    links: vec![],
                },
                DocxElement::Header {
                    kind: HeaderKind::Default,
                    content: vec![DocxElement::Paragraph {
                        text: "page header".into(),
                        parent_heading: None,
                        links: vec![],
                    }],
                },
                DocxElement::Footer {
                    kind: HeaderKind::Default,
                    content: vec![DocxElement::Paragraph {
                        text: "page footer".into(),
                        parent_heading: None,
                        links: vec![],
                    }],
                },
            ]
        );
    }

    #[test]
    fn paragraph_text_concatenates_runs_and_hyperlink_text_in_document_order() {
        // Before IO-Cycle 2 the paragraph's visible text was just the
        // runs OUTSIDE hyperlinks. Phase 2 now preserves inline order, so
        // a paragraph with runs A | <hyperlink>B</hyperlink> | C must
        // surface "ABC" as the paragraph's text — the link text is part
        // of the visible flow, the URL is metadata layered on top.
        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let p = RawParagraph {
            content: vec![
                RawInline::Run(run("before ")),
                RawInline::Hyperlink(RawHyperlink {
                    rel_id: None,
                    anchor: None,
                    runs: vec![run("link")],
                }),
                RawInline::Run(run(" after")),
            ],
            ..Default::default()
        };

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(p)],
        };
        let elements = classifier.classify(&body).unwrap();

        match &elements[0] {
            DocxElement::Paragraph { text, .. } => {
                assert_eq!(text, "before link after");
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn paragraph_with_external_hyperlink_populates_links_span_and_does_not_emit_satellite() {
        use crate::ooxml::relationships::RelationshipMap;
        use crate::pipeline::element::LinkSpan;
        use crate::raw::paragraphs::RawHyperlink;

        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let rels_xml = br#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/>
</Relationships>"#;
        let rels = RelationshipMap::parse(rels_xml, "word/_rels/document.xml.rels").unwrap();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering).with_relationships(&rels);

        let mut p = paragraph_with(None, vec![run("body")]);
        p.content.push(RawInline::Hyperlink(RawHyperlink {
            rel_id: Some("rId5".into()),
            anchor: None,
            runs: vec![run("click here")],
        }));

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(p)],
        };
        let elements = classifier.classify(&body).unwrap();

        // After IO-Cycle 3 the hyperlink is attached to the paragraph
        // as a LinkSpan and the URL is no longer emitted as a separate
        // satellite element.
        assert_eq!(
            elements,
            vec![DocxElement::Paragraph {
                text: "bodyclick here".into(),
                parent_heading: None,
                links: vec![LinkSpan {
                    text: "click here".into(),
                    url: "https://example.com".into(),
                }],
            },]
        );
    }

    #[test]
    fn classifier_detects_heading_via_outline_lvl_when_name_does_not_match() {
        // Style "MyCustom" carries outlineLvl=2 but its name is NOT
        // "heading N" — the legacy string-match heuristic would miss it.
        // After cycle 5 the classifier asks StyleResolver, which derives
        // heading_level=3 from outline_level=2.
        let mut styles = StyleTable::new();
        styles.insert(StyleEntry {
            style_id: "MyCustom".into(),
            name: "Section Heading".into(),
            style_type: StyleType::Paragraph,
            based_on: None,
            next_style: None,
            is_default: false,
            paragraph_properties: Some(RawParagraphProperties {
                outline_level: Some(2),
                ..Default::default()
            }),
            run_properties: None,
        });
        let numbering = NumberingDefs::new();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(paragraph_with(
                Some("MyCustom"),
                vec![run("Custom Section")],
            ))],
        };

        let elements = classifier.classify(&body).unwrap();
        assert_eq!(
            elements,
            vec![DocxElement::Heading {
                level: 3,
                text: "Custom Section".into(),
            }]
        );
    }

    #[test]
    fn paragraph_with_heading2_style_becomes_heading_level_2() {
        let mut styles = StyleTable::new();
        styles.insert(paragraph_style("Heading2", "heading 2"));
        let numbering = NumberingDefs::new();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(paragraph_with(
                Some("Heading2"),
                vec![run("Section")],
            ))],
        };

        let elements = classifier.classify(&body).unwrap();
        assert_eq!(
            elements,
            vec![DocxElement::Heading {
                level: 2,
                text: "Section".into()
            }]
        );
    }

    #[test]
    fn styleless_paragraph_with_one_run_becomes_paragraph_element() {
        let styles = StyleTable::new();
        let numbering = NumberingDefs::new();
        let mut classifier = ClassifierPipeline::new(&styles, &numbering);

        let body = RawBody {
            items: vec![RawBodyItem::Paragraph(paragraph_with_runs(vec![run(
                "hello",
            )]))],
        };

        let elements = classifier.classify(&body).expect("classify");

        assert_eq!(
            elements,
            vec![DocxElement::Paragraph {
                text: "hello".into(),
                parent_heading: None,
                links: vec![],
            }]
        );
    }
}
