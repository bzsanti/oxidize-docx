use crate::error::Result;
use crate::numbering::{NumberingDefs, NumberingResolver};
use crate::pipeline::element::{DocxElement, HeadingContext};
use crate::pipeline::table_builder::build_table;
use crate::raw::body::{RawBody, RawBodyItem};
use crate::raw::paragraphs::RawParagraph;
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
    numbering_resolver: NumberingResolver<'a>,
    footnotes: Option<&'a FootnoteMap>,
    endnotes: Option<&'a EndnoteMap>,
    comments: Option<&'a CommentMap>,
    current_heading: Option<HeadingContext>,
}

#[allow(dead_code)]
impl<'a> ClassifierPipeline<'a> {
    pub(crate) fn new(style_table: &'a StyleTable, numbering_defs: &'a NumberingDefs) -> Self {
        Self {
            style_table,
            numbering_resolver: NumberingResolver::new(numbering_defs),
            footnotes: None,
            endnotes: None,
            comments: None,
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

    pub(crate) fn classify(&mut self, body: &RawBody) -> Result<Vec<DocxElement>> {
        let mut out = Vec::with_capacity(body.items.len());
        for item in &body.items {
            if let RawBodyItem::Table(t) = item {
                out.push(DocxElement::Table {
                    rows: build_table(t),
                });
                continue;
            }
            if let RawBodyItem::Paragraph(p) = item {
                let text = paragraph_text(p);
                let footnote_refs = p.footnote_ref_ids.clone();
                let endnote_refs = p.endnote_ref_ids.clone();
                let comment_refs = p.comment_ref_ids.clone();
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

fn paragraph_text(p: &RawParagraph) -> String {
    let mut s = String::new();
    for run in &p.runs {
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
            runs,
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
            runs,
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
                },
            ]
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
            }]
        );
    }
}
