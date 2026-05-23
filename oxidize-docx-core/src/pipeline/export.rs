use crate::pipeline::element::DocxElement;

/// Renders a sequence of `DocxElement`s as unformatted plain text.
///
/// Blocks (paragraphs, headings, list items, tables) are separated by a
/// single blank line. Tables collapse to one line per row with cells
/// joined by ` | `. Inline formatting (bold, italic, hyperlinks) is
/// already absent at this layer of the pipeline and so does not need
/// to be stripped.
#[allow(dead_code)]
pub fn to_plain_text(elements: &[DocxElement]) -> String {
    let mut out = String::new();
    let mut prev_was_list = false;
    for elem in elements {
        let rendered = render_element(elem);
        let is_list = matches!(elem, DocxElement::ListItem { .. });
        if !out.is_empty() {
            // Tight list: single newline between consecutive list items in
            // the same run. Any other adjacency gets a blank line.
            let sep = if is_list && prev_was_list {
                "\n"
            } else {
                "\n\n"
            };
            out.push_str(sep);
        }
        out.push_str(&rendered);
        prev_was_list = is_list;
    }
    out
}

/// Renders a sequence of `DocxElement`s as GitHub-flavored Markdown.
///
/// Headings use `#` prefixes (1..=6 native; levels deeper than 6 are
/// clamped to 6 since Markdown has no `#######`). Paragraphs flow as
/// plain text separated by blank lines. List items indent by 2 spaces
/// per nesting level, with `1.` / `2.` … for decimal lists and `-` for
/// everything else. Tables emit the GFM pipe syntax with row 0 treated
/// as the header.
#[allow(dead_code)]
pub fn to_markdown(elements: &[DocxElement]) -> String {
    let mut out = String::new();
    let mut prev_was_list = false;
    for elem in elements {
        let rendered = match elem {
            DocxElement::Heading { level, text } => {
                let clamped = (*level).clamp(1, 6) as usize;
                Some(format!("{} {}", "#".repeat(clamped), text))
            }
            DocxElement::Paragraph { text, .. } => Some(text.clone()),
            DocxElement::ListItem {
                text,
                level,
                list_type,
                display_index,
            } => Some(render_list_item_md(text, *level, list_type, *display_index)),
            DocxElement::Table { rows } => Some(render_table_md(rows)),
            DocxElement::Footnote { id, text } => Some(format!("[^{id}]: {text}")),
            DocxElement::Endnote { id, text } => Some(format!("[^endnote{id}]: {text}")),
            DocxElement::Comment { id, author, text } => {
                Some(format!("> **Comment {id} ({author}):** {text}"))
            }
            DocxElement::Hyperlink { .. } => None,
        };
        let Some(rendered) = rendered else {
            continue;
        };
        let is_list = matches!(elem, DocxElement::ListItem { .. });
        if !out.is_empty() {
            let sep = if is_list && prev_was_list {
                "\n"
            } else {
                "\n\n"
            };
            out.push_str(sep);
        }
        out.push_str(&rendered);
        prev_was_list = is_list;
    }
    out
}

fn render_table_md(rows: &[crate::pipeline::element::TableRow]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let header = &rows[0];
    let col_count = header.cells.len();

    out.push_str(&format_md_row(header));
    if col_count > 0 {
        out.push('\n');
        out.push('|');
        for _ in 0..col_count {
            out.push_str(" --- |");
        }
    }
    for row in &rows[1..] {
        out.push('\n');
        out.push_str(&format_md_row(row));
    }
    out
}

fn format_md_row(row: &crate::pipeline::element::TableRow) -> String {
    let mut s = String::from("|");
    for cell in &row.cells {
        s.push(' ');
        s.push_str(&cell.text);
        s.push_str(" |");
    }
    s
}

fn render_list_item_md(
    text: &str,
    level: u8,
    list_type: &crate::numbering::ListType,
    display_index: Option<u32>,
) -> String {
    let indent = " ".repeat(level as usize * 2);
    let marker = match (list_type, display_index) {
        (crate::numbering::ListType::Decimal, Some(idx)) => format!("{idx}."),
        _ => "-".to_string(),
    };
    format!("{indent}{marker} {text}")
}

fn render_element(elem: &DocxElement) -> String {
    match elem {
        DocxElement::Paragraph { text, .. } => text.clone(),
        DocxElement::Heading { text, .. } => text.clone(),
        DocxElement::ListItem { text, .. } => text.clone(),
        DocxElement::Table { rows } => rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|c| c.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        DocxElement::Footnote { id, text } => format!("[{id}] {text}"),
        DocxElement::Endnote { id, text } => format!("[endnote {id}] {text}"),
        DocxElement::Comment { id, author, text } => format!("[comment {id} by {author}] {text}"),
        DocxElement::Hyperlink { .. } => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_table_with_two_rows_emits_header_separator_and_data_row() {
        use crate::pipeline::element::{TableCell, TableRow};
        let elements = vec![DocxElement::Table {
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
                            col_span: 1,
                            row_span: 1,
                        },
                    ],
                },
                TableRow {
                    cells: vec![
                        TableCell {
                            text: "C".into(),
                            col_span: 1,
                            row_span: 1,
                        },
                        TableCell {
                            text: "D".into(),
                            col_span: 1,
                            row_span: 1,
                        },
                    ],
                },
            ],
        }];
        assert_eq!(
            to_markdown(&elements),
            "| A | B |\n| --- | --- |\n| C | D |"
        );
    }

    #[test]
    fn markdown_list_items_indent_by_two_spaces_per_level_and_use_bullet_or_number() {
        use crate::numbering::ListType;
        let elements = vec![
            DocxElement::ListItem {
                text: "A".into(),
                level: 0,
                list_type: ListType::Decimal,
                display_index: Some(1),
            },
            DocxElement::ListItem {
                text: "A.1".into(),
                level: 1,
                list_type: ListType::Bullet,
                display_index: None,
            },
            DocxElement::ListItem {
                text: "B".into(),
                level: 0,
                list_type: ListType::Decimal,
                display_index: Some(2),
            },
        ];
        assert_eq!(to_markdown(&elements), "1. A\n  - A.1\n2. B");
    }

    #[test]
    fn markdown_heading_followed_by_paragraph_uses_blank_line_separator() {
        let elements = vec![
            DocxElement::Heading {
                level: 2,
                text: "S".into(),
            },
            DocxElement::Paragraph {
                text: "body".into(),
                parent_heading: None,
            },
        ];
        assert_eq!(to_markdown(&elements), "## S\n\nbody");
    }

    #[test]
    fn markdown_two_paragraphs_are_joined_by_blank_line() {
        let elements = vec![
            DocxElement::Paragraph {
                text: "first".into(),
                parent_heading: None,
            },
            DocxElement::Paragraph {
                text: "second".into(),
                parent_heading: None,
            },
        ];
        assert_eq!(to_markdown(&elements), "first\n\nsecond");
    }

    #[test]
    fn markdown_heading_levels_1_through_6_emit_corresponding_hash_prefix() {
        for level in 1u8..=6 {
            let elements = vec![DocxElement::Heading {
                level,
                text: "Section".into(),
            }];
            let prefix = "#".repeat(level as usize);
            assert_eq!(to_markdown(&elements), format!("{prefix} Section"));
        }
    }

    #[test]
    fn list_items_are_tight_and_table_rows_are_pipe_separated() {
        use crate::numbering::ListType;
        use crate::pipeline::element::{TableCell, TableRow};

        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "Section".into(),
            },
            DocxElement::ListItem {
                text: "item1".into(),
                level: 0,
                list_type: ListType::Decimal,
                display_index: Some(1),
            },
            DocxElement::ListItem {
                text: "item2".into(),
                level: 0,
                list_type: ListType::Decimal,
                display_index: Some(2),
            },
            DocxElement::Table {
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
                                col_span: 1,
                                row_span: 1,
                            },
                        ],
                    },
                    TableRow {
                        cells: vec![
                            TableCell {
                                text: "C".into(),
                                col_span: 1,
                                row_span: 1,
                            },
                            TableCell {
                                text: "D".into(),
                                col_span: 1,
                                row_span: 1,
                            },
                        ],
                    },
                ],
            },
        ];

        // Heading and paragraph/table blocks separated by blank line.
        // Consecutive list items use single newline (tight list).
        // Table cells joined by " | ", rows by "\n" within the block.
        assert_eq!(
            to_plain_text(&elements),
            "Section\n\nitem1\nitem2\n\nA | B\nC | D"
        );
    }

    #[test]
    fn heading_and_paragraph_are_joined_by_blank_line() {
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "Intro".into(),
            },
            DocxElement::Paragraph {
                text: "body".into(),
                parent_heading: None,
            },
        ];
        assert_eq!(to_plain_text(&elements), "Intro\n\nbody");
    }

    #[test]
    fn two_paragraphs_are_joined_by_blank_line() {
        let elements = vec![
            DocxElement::Paragraph {
                text: "first".into(),
                parent_heading: None,
            },
            DocxElement::Paragraph {
                text: "second".into(),
                parent_heading: None,
            },
        ];
        assert_eq!(to_plain_text(&elements), "first\n\nsecond");
    }

    #[test]
    fn single_paragraph_yields_its_text_verbatim() {
        let elements = vec![DocxElement::Paragraph {
            text: "hello".into(),
            parent_heading: None,
        }];
        assert_eq!(to_plain_text(&elements), "hello");
    }
}
