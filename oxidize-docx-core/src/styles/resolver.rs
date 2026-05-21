use std::collections::HashSet;

use crate::error::{DocxError, Result};
use crate::raw::paragraphs::RawParagraph;
use crate::raw::runs::{RawRun, RawRunProperties};
use crate::styles::formatting::ResolvedFormatting;
use crate::styles::table::{StyleEntry, StyleTable};

/// Maximum depth of a `basedOn` chain. Beyond this, `collect_style_chain`
/// returns `DocxError::StyleChainTooDeep`. Real-world enterprise documents
/// (Confluence/Notion exports) reach 8–10 levels; 64 absorbs that with
/// generous headroom while still bounding memory and recursion-equivalent
/// loops in malformed inputs.
pub(crate) const MAX_STYLE_DEPTH: usize = 64;

#[allow(dead_code)]
pub(crate) struct StyleResolver<'a> {
    table: &'a StyleTable,
}

#[allow(dead_code)]
impl<'a> StyleResolver<'a> {
    pub(crate) fn new(table: &'a StyleTable) -> Self {
        Self { table }
    }

    pub(crate) fn resolve_run(
        &self,
        paragraph: &RawParagraph,
        run: &RawRun,
    ) -> Result<ResolvedFormatting> {
        let mut resolved = ResolvedFormatting::default();

        if let Some(defaults) = self.table.doc_defaults_run_properties() {
            merge_run_props_into(&mut resolved, defaults);
        }

        if let Some(style_id) = paragraph.properties.style_id.as_deref() {
            for style in self.collect_style_chain(style_id)? {
                if let Some(rpr) = &style.run_properties {
                    merge_run_props_into(&mut resolved, rpr);
                }
            }
        }

        merge_run_props_into(&mut resolved, &run.properties);

        Ok(resolved)
    }

    /// Returns the basedOn chain ordered root-first.
    /// Example: Title basedOn Heading1 basedOn Normal → [Normal, Heading1, Title].
    ///
    /// Errors:
    /// - `CircularStyleReference` if a style is visited twice (cycle).
    /// - `StyleChainTooDeep` if the chain length exceeds `MAX_STYLE_DEPTH`;
    ///   `style` field carries the original `style_id` argument.
    fn collect_style_chain(&self, style_id: &str) -> Result<Vec<&'a StyleEntry>> {
        let mut chain = Vec::new();
        let mut visited: HashSet<&str> = HashSet::new();
        let mut current_id = style_id;

        loop {
            if chain.len() >= MAX_STYLE_DEPTH {
                return Err(DocxError::StyleChainTooDeep {
                    style: style_id.to_string(),
                    limit: MAX_STYLE_DEPTH,
                });
            }
            if !visited.insert(current_id) {
                return Err(DocxError::CircularStyleReference(current_id.to_string()));
            }
            let entry = match self.table.get(current_id) {
                Some(e) => e,
                None => break,
            };
            chain.push(entry);
            match entry.based_on.as_deref() {
                Some(parent) => current_id = parent,
                None => break,
            }
        }

        chain.reverse();
        Ok(chain)
    }
}

/// Layered merge: `src` overrides `target`. A property is considered "set"
/// when its Option is Some (for value-bearing fields) or its bool is true
/// (for bool fields — current Phase 2 RawRunProperties does not preserve
/// explicit `w:val="0"`, so explicit-false override is not yet supported).
fn merge_run_props_into(target: &mut ResolvedFormatting, src: &RawRunProperties) {
    if let Some(size) = src.font_size_half_points {
        target.font_size_half_points = Some(size);
    }
    if let Some(color) = &src.color {
        target.color = Some(color.clone());
    }
    if src.bold {
        target.bold = true;
    }
    if src.italic {
        target.italic = true;
    }
    if src.underline {
        target.underline = true;
    }
    if src.strikethrough {
        target.strikethrough = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DocxError;
    use crate::raw::paragraphs::RawParagraphProperties;
    use crate::raw::runs::RawRunProperties;
    use crate::styles::table::{StyleEntry, StyleType};

    fn paragraph_style(
        id: &str,
        based_on: Option<&str>,
        rpr: Option<RawRunProperties>,
    ) -> StyleEntry {
        StyleEntry {
            style_id: id.into(),
            name: id.into(),
            style_type: StyleType::Paragraph,
            based_on: based_on.map(|s| s.into()),
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: rpr,
        }
    }

    #[test]
    fn run_inherits_font_size_from_doc_defaults() {
        let mut table = StyleTable::new();
        table.doc_defaults_run = Some(RawRunProperties {
            font_size_half_points: Some(24),
            ..Default::default()
        });
        let resolver = StyleResolver::new(&table);

        let paragraph = RawParagraph::default();
        let run = RawRun {
            text: Some("hello".into()),
            properties: RawRunProperties::default(),
        };

        let resolved = resolver.resolve_run(&paragraph, &run).unwrap();
        assert_eq!(resolved.font_size_half_points, Some(24));
    }

    #[test]
    fn run_inherits_through_based_on_chain_three_levels() {
        let mut table = StyleTable::new();
        table.insert(paragraph_style(
            "Normal",
            None,
            Some(RawRunProperties {
                font_size_half_points: Some(22),
                ..Default::default()
            }),
        ));
        table.insert(paragraph_style(
            "Heading1",
            Some("Normal"),
            Some(RawRunProperties {
                color: Some("FF0000".into()),
                ..Default::default()
            }),
        ));
        table.insert(paragraph_style(
            "Title",
            Some("Heading1"),
            Some(RawRunProperties {
                font_size_half_points: Some(36),
                ..Default::default()
            }),
        ));

        let resolver = StyleResolver::new(&table);
        let paragraph = RawParagraph {
            properties: RawParagraphProperties {
                style_id: Some("Title".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let run = RawRun {
            text: Some("h".into()),
            properties: RawRunProperties::default(),
        };

        let resolved = resolver.resolve_run(&paragraph, &run).unwrap();
        assert_eq!(
            resolved.font_size_half_points,
            Some(36),
            "Title should override font_size"
        );
        assert_eq!(
            resolved.color.as_deref(),
            Some("FF0000"),
            "Heading1's color should propagate to Title"
        );
    }

    #[test]
    fn chain_deeper_than_max_returns_style_chain_too_deep() {
        // Build a linear chain Style0 ← Style1 ← ... ← Style70.
        // Style0 has no parent; each Style(N) is based_on Style(N-1).
        // Max depth is 64; a chain of 71 distinct styles must exceed it.
        let mut table = StyleTable::new();
        table.insert(paragraph_style("Style0", None, None));
        for i in 1..=70u32 {
            let parent = format!("Style{}", i - 1);
            let id = format!("Style{i}");
            table.insert(paragraph_style(&id, Some(&parent), None));
        }

        let resolver = StyleResolver::new(&table);
        let paragraph = RawParagraph {
            properties: RawParagraphProperties {
                style_id: Some("Style70".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let run = RawRun {
            text: Some("x".into()),
            properties: RawRunProperties::default(),
        };

        let result = resolver.resolve_run(&paragraph, &run);
        match result {
            Err(DocxError::StyleChainTooDeep { style, limit }) => {
                assert_eq!(limit, 64, "expected MAX_STYLE_DEPTH=64, got {limit}");
                assert_eq!(style, "Style70", "expected starting style in error");
            }
            Ok(r) => panic!("expected StyleChainTooDeep, got Ok({r:?})"),
            Err(e) => panic!("expected StyleChainTooDeep, got {e:?}"),
        }
    }

    #[test]
    fn cyclic_based_on_chain_returns_circular_style_reference() {
        let mut table = StyleTable::new();
        // A basedOn B basedOn A — direct 2-style cycle
        table.insert(paragraph_style("A", Some("B"), None));
        table.insert(paragraph_style("B", Some("A"), None));

        let resolver = StyleResolver::new(&table);
        let paragraph = RawParagraph {
            properties: RawParagraphProperties {
                style_id: Some("A".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let run = RawRun {
            text: Some("x".into()),
            properties: RawRunProperties::default(),
        };

        let result = resolver.resolve_run(&paragraph, &run);
        match result {
            Err(DocxError::CircularStyleReference(id)) => {
                assert!(
                    id == "A" || id == "B",
                    "expected cycle to be reported on A or B, got '{id}'"
                );
            }
            Ok(r) => panic!("expected CircularStyleReference, got Ok({r:?})"),
            Err(e) => panic!("expected CircularStyleReference, got {e:?}"),
        }
    }
}
