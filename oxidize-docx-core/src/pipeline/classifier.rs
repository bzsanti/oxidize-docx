use crate::error::Result;
use crate::numbering::{NumberingDefs, NumberingResolver};
use crate::pipeline::element::{DocxElement, HeadingContext};
use crate::raw::body::{RawBody, RawBodyItem};
use crate::raw::paragraphs::RawParagraph;
use crate::styles::table::StyleTable;

/// Walks a `RawBody` in document order and emits a `Vec<DocxElement>`.
/// Stateful across calls within the same instance — heading context and
/// list counters carry from one paragraph to the next.
#[allow(dead_code)]
pub(crate) struct ClassifierPipeline<'a> {
    style_table: &'a StyleTable,
    numbering_resolver: NumberingResolver<'a>,
    current_heading: Option<HeadingContext>,
}

#[allow(dead_code)]
impl<'a> ClassifierPipeline<'a> {
    pub(crate) fn new(style_table: &'a StyleTable, numbering_defs: &'a NumberingDefs) -> Self {
        Self {
            style_table,
            numbering_resolver: NumberingResolver::new(numbering_defs),
            current_heading: None,
        }
    }

    pub(crate) fn classify(&mut self, body: &RawBody) -> Result<Vec<DocxElement>> {
        let mut out = Vec::with_capacity(body.items.len());
        for item in &body.items {
            if let RawBodyItem::Paragraph(p) = item {
                let text = paragraph_text(p);
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
                    match self.heading_level(p) {
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
            }
        }
        Ok(out)
    }

    /// Returns 1..=9 if the paragraph's style name matches Word's
    /// `heading N` convention (case-insensitive).
    fn heading_level(&self, p: &RawParagraph) -> Option<u8> {
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
                indent_left: None,
                indent_hanging: None,
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
                indent_left: None,
                indent_hanging: None,
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
