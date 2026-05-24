use std::collections::HashSet;

use crate::error::{DocxError, Result};
use crate::numbering::defs::NumberingLevel;
use crate::raw::paragraphs::{RawParagraph, RawParagraphProperties};
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

    /// Resolves paragraph-level formatting through the style chain.
    /// Layers applied in order (each overrides the previous):
    ///   1. `docDefaults` pPr
    ///   2. `basedOn` chain pPr (root-first)
    ///   3. direct pPr on the paragraph
    ///
    /// The list-level layer is intentionally not consulted here — it is
    /// applied by `resolve_run` together with rPr resolution, since
    /// list-level properties primarily affect run-level rendering and
    /// would require numbering context the paragraph alone doesn't have.
    pub(crate) fn resolve_paragraph(&self, paragraph: &RawParagraph) -> Result<ResolvedFormatting> {
        let mut resolved = ResolvedFormatting::default();

        if let Some(defaults) = &self.table.doc_defaults_paragraph {
            merge_paragraph_props_into(&mut resolved, defaults);
        }

        if let Some(style_id) = paragraph.properties.style_id.as_deref() {
            for style in self.collect_style_chain(style_id)? {
                if let Some(ppr) = &style.paragraph_properties {
                    merge_paragraph_props_into(&mut resolved, ppr);
                }
            }
        }

        merge_paragraph_props_into(&mut resolved, &paragraph.properties);

        // OOXML outlineLvl 0..=8 → heading 1..=9. outlineLvl=9 is "body
        // text" — an explicit non-heading declaration that we must NOT
        // map to a heading_level. Anything else (None, out-of-range)
        // leaves heading_level as None.
        resolved.heading_level = match resolved.outline_level {
            Some(lvl @ 0..=8) => Some(lvl + 1),
            _ => None,
        };

        Ok(resolved)
    }

    /// Resolves the run's rPr through the full 4-layer chain:
    ///   1. `docDefaults` rPr
    ///   2. `basedOn` chain rPr (root-first)
    ///   3. list-level rPr (from the `<w:lvl>` matching the paragraph's numPr)
    ///   4. direct rPr on the run
    ///
    /// Pass `list_level = None` when the paragraph carries no numPr or
    /// when the level lookup failed for any reason. Callers that already
    /// resolved the level via `NumberingResolver` can hand it in directly.
    pub(crate) fn resolve_run(
        &self,
        paragraph: &RawParagraph,
        run: &RawRun,
        list_level: Option<&NumberingLevel>,
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

        if let Some(level) = list_level {
            if let Some(rpr) = &level.run_properties {
                merge_run_props_into(&mut resolved, rpr);
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

/// Layered merge for paragraph properties. Mirrors `merge_run_props_into`
/// semantics: `Some(_)` overrides; `None` leaves the existing value.
fn merge_paragraph_props_into(target: &mut ResolvedFormatting, src: &RawParagraphProperties) {
    if let Some(lvl) = src.outline_level {
        target.outline_level = Some(lvl);
    }
}

/// Layered merge: `src` overrides `target`. A property is considered "set"
/// when its Option is Some. For the toggle props this honors OOXML override
/// semantics: `Some(true)` forces ON, `Some(false)` (from `<w:b w:val="0"/>`)
/// forces OFF over an inherited true, and `None` leaves the layer below
/// untouched.
fn merge_run_props_into(target: &mut ResolvedFormatting, src: &RawRunProperties) {
    if let Some(size) = src.font_size_half_points {
        target.font_size_half_points = Some(size);
    }
    if let Some(color) = &src.color {
        target.color = Some(color.clone());
    }
    if let Some(bold) = src.bold {
        target.bold = bold;
    }
    if let Some(italic) = src.italic {
        target.italic = italic;
    }
    if let Some(underline) = src.underline {
        target.underline = underline;
    }
    if let Some(strikethrough) = src.strikethrough {
        target.strikethrough = strikethrough;
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

        let resolved = resolver.resolve_run(&paragraph, &run, None).unwrap();
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

        let resolved = resolver.resolve_run(&paragraph, &run, None).unwrap();
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

        let result = resolver.resolve_run(&paragraph, &run, None);
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
    fn resolve_paragraph_derives_heading_level_from_outline_level() {
        // OOXML outlineLvl semantics: 0..=8 are heading 1..=9; outlineLvl=9
        // explicitly means "body text" — paragraph is NOT a heading despite
        // having an outline_level set.
        fn style_with_outline(id: &str, lvl: u8) -> StyleEntry {
            StyleEntry {
                style_id: id.into(),
                name: id.into(),
                style_type: StyleType::Paragraph,
                based_on: None,
                next_style: None,
                is_default: false,
                paragraph_properties: Some(RawParagraphProperties {
                    outline_level: Some(lvl),
                    ..Default::default()
                }),
                run_properties: None,
            }
        }
        fn paragraph_styled(style_id: &str) -> RawParagraph {
            RawParagraph {
                properties: RawParagraphProperties {
                    style_id: Some(style_id.into()),
                    ..Default::default()
                },
                ..Default::default()
            }
        }

        let mut table = StyleTable::new();
        table.insert(style_with_outline("H1", 0));
        table.insert(style_with_outline("H9", 8));
        table.insert(style_with_outline("Body", 9));

        let resolver = StyleResolver::new(&table);

        let h1 = resolver.resolve_paragraph(&paragraph_styled("H1")).unwrap();
        assert_eq!(
            h1.heading_level,
            Some(1),
            "outline_level=0 → heading_level=1"
        );

        let h9 = resolver.resolve_paragraph(&paragraph_styled("H9")).unwrap();
        assert_eq!(
            h9.heading_level,
            Some(9),
            "outline_level=8 → heading_level=9"
        );

        let body = resolver
            .resolve_paragraph(&paragraph_styled("Body"))
            .unwrap();
        assert_eq!(body.outline_level, Some(9), "outline_level still recorded");
        assert_eq!(
            body.heading_level, None,
            "outline_level=9 is body text, not a heading"
        );

        // No outlineLvl at all → no heading_level either.
        let plain = resolver
            .resolve_paragraph(&RawParagraph::default())
            .unwrap();
        assert_eq!(plain.heading_level, None);
    }

    #[test]
    fn resolve_run_applies_list_level_rpr_between_chain_and_direct() {
        // Layer 2 (basedOn chain): Normal style sets color=FF0000 via rPr.
        // Layer 3 (list-level):   <w:lvl>/<w:rPr> sets color=00FF00.
        // Layer 4 (direct):       run.rPr has no color.
        // Result: list-level wins (3 > 2, and 4 doesn't override since None).
        use crate::numbering::defs::NumberingLevel;

        let mut table = StyleTable::new();
        table.insert(StyleEntry {
            style_id: "Normal".into(),
            name: "Normal".into(),
            style_type: StyleType::Paragraph,
            based_on: None,
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: Some(RawRunProperties {
                color: Some("FF0000".into()),
                ..Default::default()
            }),
        });
        let level = NumberingLevel {
            ilvl: 0,
            start: 1,
            num_fmt: "decimal".into(),
            level_text: "%1.".into(),
            run_properties: Some(RawRunProperties {
                color: Some("00FF00".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let resolver = StyleResolver::new(&table);
        let paragraph = RawParagraph {
            properties: RawParagraphProperties {
                style_id: Some("Normal".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let run = RawRun {
            text: Some("item".into()),
            properties: RawRunProperties::default(),
        };

        let resolved = resolver
            .resolve_run(&paragraph, &run, Some(&level))
            .unwrap();
        assert_eq!(
            resolved.color.as_deref(),
            Some("00FF00"),
            "list-level rPr must override basedOn chain rPr (layer 3 > layer 2)"
        );
    }

    #[test]
    fn resolve_paragraph_inherits_outline_level_through_chain() {
        // Normal owns pPr with outlineLvl=2. Heading1 is basedOn Normal
        // but declares no pPr of its own. A paragraph whose style is
        // Heading1 must inherit outline_level=2 from the chain.
        let mut table = StyleTable::new();
        table.insert(StyleEntry {
            style_id: "Normal".into(),
            name: "Normal".into(),
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
        table.insert(StyleEntry {
            style_id: "Heading1".into(),
            name: "heading 1".into(),
            style_type: StyleType::Paragraph,
            based_on: Some("Normal".into()),
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: None,
        });

        let resolver = StyleResolver::new(&table);
        let paragraph = RawParagraph {
            properties: RawParagraphProperties {
                style_id: Some("Heading1".into()),
                ..Default::default()
            },
            ..Default::default()
        };

        let resolved = resolver.resolve_paragraph(&paragraph).unwrap();
        assert_eq!(
            resolved.outline_level,
            Some(2),
            "Heading1 inherits outline_level=2 from Normal via basedOn"
        );
    }

    #[test]
    fn child_run_explicit_false_overrides_inherited_true_bold() {
        // OOXML: <w:b w:val="0"/> in a run's rPr must turn OFF a bold
        // inherited from the paragraph style. While RawRunProperties used a
        // bare `bool` the explicit-false signal was indistinguishable from
        // "absent", so the inherited true survived and the resolved run
        // rendered bold when it should have been plain. The fix moves all
        // toggle props to Option<bool>: Some(true) = explicit on, Some(false)
        // = explicit off, None = inherit from layer below.
        let mut table = StyleTable::new();
        table.insert(StyleEntry {
            style_id: "Strong".into(),
            name: "Strong".into(),
            style_type: StyleType::Paragraph,
            based_on: None,
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: Some(RawRunProperties {
                bold: Some(true),
                ..Default::default()
            }),
        });

        let resolver = StyleResolver::new(&table);
        let paragraph = RawParagraph {
            properties: RawParagraphProperties {
                style_id: Some("Strong".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let run = RawRun {
            text: Some("plain".into()),
            properties: RawRunProperties {
                bold: Some(false),
                ..Default::default()
            },
        };

        let resolved = resolver.resolve_run(&paragraph, &run, None).unwrap();
        assert!(
            !resolved.bold,
            "explicit <w:b w:val=\"0\"/> in run must override inherited bold=true"
        );
    }

    #[test]
    fn child_none_preserves_inherited_toggle_while_some_false_overrides() {
        // Complement to child_run_explicit_false_overrides_inherited_true_bold:
        // a child whose toggle is None must leave the inherited value intact,
        // while a Some(false) on a *different* field still overrides. Using
        // two fields guards against the merge accidentally writing the wrong
        // target field.
        let mut table = StyleTable::new();
        table.insert(StyleEntry {
            style_id: "Emphatic".into(),
            name: "Emphatic".into(),
            style_type: StyleType::Paragraph,
            based_on: None,
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: Some(RawRunProperties {
                bold: Some(true),
                italic: Some(true),
                ..Default::default()
            }),
        });

        let resolver = StyleResolver::new(&table);
        let paragraph = RawParagraph {
            properties: RawParagraphProperties {
                style_id: Some("Emphatic".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        // Child turns bold OFF explicitly, says nothing about italic.
        let run = RawRun {
            text: Some("x".into()),
            properties: RawRunProperties {
                bold: Some(false),
                italic: None,
                ..Default::default()
            },
        };

        let resolved = resolver.resolve_run(&paragraph, &run, None).unwrap();
        assert!(
            !resolved.bold,
            "Some(false) child must override inherited bold"
        );
        assert!(
            resolved.italic,
            "None child must preserve inherited italic=true"
        );
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

        let result = resolver.resolve_run(&paragraph, &run, None);
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
