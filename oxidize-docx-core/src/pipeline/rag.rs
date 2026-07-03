use std::borrow::Cow;

use crate::pipeline::chunk_metadata::{content_type_flags, heading_path_from, ContentTypeFlags};
use crate::pipeline::element::{DocxElement, HeadingContext};
use crate::pipeline::profile::ExtractionProfile;

/// A semantically-bounded slice of the document ready for ingestion into
/// a RAG pipeline. `paragraph_indices` references positions in the
/// original `Vec<DocxElement>` so callers can correlate chunks back to
/// source material; `heading_context` is the stack of headings active
/// at the chunk's location (deepest first), giving downstream rerankers
/// enough structural context without re-walking the document.
///
/// `token_estimate` is `word_count * 1.5` — a crude approximation that
/// trades accuracy for portability (no tokenizer dependency). Consumers
/// targeting a specific embedding model should treat it as an upper
/// bound and re-tokenize if precision matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RagChunk {
    pub text: String,
    pub paragraph_indices: Vec<usize>,
    pub element_types: Vec<String>,
    pub heading_context: Vec<HeadingContext>,
    pub token_estimate: usize,
    pub is_oversized: bool,
    pub content_types: ContentTypeFlags,
    pub heading_path: Vec<String>,
    pub full_text: String,
}

/// Hybrid chunker that walks a `Vec<DocxElement>` in document order and
/// emits `RagChunk`s sized for embedding APIs. The strategy is
/// heading-aware (a heading change opens a new chunk) and size-aware
/// (the running token estimate caps each chunk).
#[derive(Debug, Clone)]
pub struct DocxRagChunker {
    pub max_tokens: usize,
    pub profile: ExtractionProfile,
    pub merge_policy: crate::pipeline::hybrid::MergePolicy,
    pub group_sections: bool,
}

impl Default for DocxRagChunker {
    fn default() -> Self {
        Self {
            max_tokens: 800,
            profile: ExtractionProfile::default(),
            merge_policy: crate::pipeline::hybrid::MergePolicy::default(),
            group_sections: false,
        }
    }
}

#[allow(dead_code)]
impl DocxRagChunker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_tokens(mut self, n: usize) -> Self {
        self.max_tokens = n;
        self
    }

    pub fn with_profile(mut self, profile: ExtractionProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_merge_policy(mut self, policy: crate::pipeline::hybrid::MergePolicy) -> Self {
        self.merge_policy = policy;
        self
    }

    pub fn with_section_grouping(mut self, on: bool) -> Self {
        self.group_sections = on;
        self
    }

    pub fn chunk(&self, elements: &[DocxElement]) -> Vec<RagChunk> {
        let view = apply_profile(elements, self.profile);
        self.chunk_view(view.as_ref())
    }

    fn chunk_view(&self, elements: &[DocxElement]) -> Vec<RagChunk> {
        if self.group_sections {
            crate::pipeline::hybrid::pack_grouped(elements, self.max_tokens, self.merge_policy)
        } else {
            crate::pipeline::hybrid::pack(elements, self.max_tokens, self.merge_policy)
        }
    }
}

/// Pre-transforms the element stream according to the active extraction
/// profile. `Default` and `Technical` borrow the original slice for free;
/// `Minimal` and `Academic` allocate a transformed `Vec` because they
/// change the element count or rewrite paragraph text.
fn apply_profile(elements: &[DocxElement], profile: ExtractionProfile) -> Cow<'_, [DocxElement]> {
    match profile {
        ExtractionProfile::Default | ExtractionProfile::Technical => Cow::Borrowed(elements),
        ExtractionProfile::Minimal => {
            let filtered: Vec<DocxElement> = elements
                .iter()
                .filter(|e| {
                    !matches!(
                        e,
                        DocxElement::Footnote { .. }
                            | DocxElement::Endnote { .. }
                            | DocxElement::Comment { .. }
                    )
                })
                .cloned()
                .collect();
            Cow::Owned(filtered)
        }
        ExtractionProfile::Academic => Cow::Owned(academic_inline(elements)),
    }
}

/// Folds every `Footnote` and `Endnote` into the trailing text of the
/// element that referenced it, so each citation rides along with its
/// host paragraph instead of becoming its own chunk. Orphan notes (no
/// preceding host element) are dropped — Academic assumes the document
/// was validly authored.
fn academic_inline(elements: &[DocxElement]) -> Vec<DocxElement> {
    let mut out: Vec<DocxElement> = Vec::with_capacity(elements.len());
    for elem in elements {
        match elem {
            DocxElement::Footnote { id, text } => {
                if let Some(last) = out.last_mut() {
                    append_text(last, &format!(" (Note {id}: {text})"));
                }
            }
            DocxElement::Endnote { id, text } => {
                if let Some(last) = out.last_mut() {
                    append_text(last, &format!(" (Endnote {id}: {text})"));
                }
            }
            other => out.push(other.clone()),
        }
    }
    out
}

fn append_text(elem: &mut DocxElement, addendum: &str) {
    match elem {
        DocxElement::Paragraph { text, .. } => text.push_str(addendum),
        DocxElement::ListItem { text, .. } => text.push_str(addendum),
        DocxElement::Heading { text, .. } => text.push_str(addendum),
        _ => {}
    }
}

fn build_full_text(heading_path: &[String], text: &str) -> String {
    if heading_path.is_empty() {
        text.to_string()
    } else {
        format!("{}\n\n{}", heading_path.join(" > "), text)
    }
}

#[derive(Default)]
pub(crate) struct ChunkAccumulator {
    text_parts: Vec<String>,
    paragraph_indices: Vec<usize>,
    element_types: Vec<String>,
}

impl ChunkAccumulator {
    pub(crate) fn is_empty(&self) -> bool {
        self.paragraph_indices.is_empty()
    }

    pub(crate) fn push(&mut self, idx: usize, text: String, etype: &str) {
        self.text_parts.push(text);
        self.paragraph_indices.push(idx);
        self.element_types.push(etype.to_string());
    }

    pub(crate) fn last_type(&self) -> Option<&str> {
        self.element_types.last().map(|s| s.as_str())
    }

    pub(crate) fn finalize(self, heading_context: Vec<HeadingContext>) -> RagChunk {
        let text = self.text_parts.join("\n\n");
        let token_estimate = crate::pipeline::hybrid::estimate_tokens(&text);
        let content_types = content_type_flags(&self.element_types);
        let heading_path = heading_path_from(&heading_context);
        let full_text = build_full_text(&heading_path, &text);
        RagChunk {
            text,
            paragraph_indices: self.paragraph_indices,
            element_types: self.element_types,
            heading_context,
            token_estimate,
            is_oversized: false,
            content_types,
            heading_path,
            full_text,
        }
    }
}

impl RagChunk {
    pub(crate) fn oversized_fragment(
        text: String,
        idx: usize,
        etype: &str,
        heading_context: Vec<HeadingContext>,
        token_estimate: usize,
    ) -> Self {
        let content_types = content_type_flags(&[etype.to_string()]);
        let heading_path = heading_path_from(&heading_context);
        let full_text = build_full_text(&heading_path, &text);
        RagChunk {
            text,
            paragraph_indices: vec![idx],
            element_types: vec![etype.to_string()],
            heading_context,
            token_estimate,
            is_oversized: true,
            content_types,
            heading_path,
            full_text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn academic_profile_inlines_footnote_text_into_preceding_paragraph() {
        let elements = vec![
            DocxElement::Paragraph {
                text: "see".into(),
                parent_heading: None,
                links: vec![],
            },
            DocxElement::Footnote {
                id: 1,
                text: "details".into(),
            },
        ];
        let chunks = DocxRagChunker::new()
            .with_profile(ExtractionProfile::Academic)
            .chunk(&elements);

        assert_eq!(chunks.len(), 1);
        let c = &chunks[0];
        assert_eq!(c.text, "see (Note 1: details)");
        assert_eq!(c.element_types, vec!["paragraph".to_string()]);
        assert_eq!(c.paragraph_indices, vec![0]);
    }

    #[test]
    fn minimal_profile_drops_footnote_endnote_and_comment_elements() {
        let elements = vec![
            DocxElement::Paragraph {
                text: "main".into(),
                parent_heading: None,
                links: vec![],
            },
            DocxElement::Footnote {
                id: 1,
                text: "fn".into(),
            },
            DocxElement::Endnote {
                id: 2,
                text: "en".into(),
            },
            DocxElement::Comment {
                id: 3,
                author: "A".into(),
                text: "cm".into(),
            },
        ];
        let chunks = DocxRagChunker::new()
            .with_profile(ExtractionProfile::Minimal)
            .chunk(&elements);

        assert_eq!(chunks.len(), 1);
        let c = &chunks[0];
        assert_eq!(c.text, "main");
        assert_eq!(c.element_types, vec!["paragraph".to_string()]);
        assert_eq!(c.paragraph_indices, vec![0]);
    }

    #[test]
    fn default_profile_produces_identical_chunks_to_no_profile_call() {
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "H".into(),
            },
            DocxElement::Paragraph {
                text: "body".into(),
                parent_heading: None,
                links: vec![],
            },
        ];
        let baseline = DocxRagChunker::new().chunk(&elements);
        let with_default = DocxRagChunker::new()
            .with_profile(ExtractionProfile::Default)
            .chunk(&elements);
        assert_eq!(baseline, with_default);
    }

    #[test]
    fn paragraph_indices_are_contiguous_within_each_chunk_and_cover_input() {
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "A".into(),
            },
            DocxElement::Paragraph {
                text: "p0".into(),
                parent_heading: None,
                links: vec![],
            },
            DocxElement::Paragraph {
                text: "p1".into(),
                parent_heading: None,
                links: vec![],
            },
            DocxElement::Heading {
                level: 1,
                text: "B".into(),
            },
            DocxElement::Paragraph {
                text: "p2".into(),
                parent_heading: None,
                links: vec![],
            },
        ];
        let chunks = DocxRagChunker::new().chunk(&elements);

        // Union of paragraph_indices across chunks (preserving order) must
        // reproduce 0..elements.len() exactly — no gap, no duplicate.
        let all: Vec<usize> = chunks
            .iter()
            .flat_map(|c| c.paragraph_indices.iter().copied())
            .collect();
        assert_eq!(all, (0..elements.len()).collect::<Vec<_>>());

        for c in &chunks {
            for w in c.paragraph_indices.windows(2) {
                assert_eq!(w[1], w[0] + 1, "indices within a chunk must be contiguous");
            }
        }
    }

    #[test]
    fn paragraph_exceeding_max_tokens_splits_at_sentence_boundaries_marked_oversized() {
        // "First sentence. Second sentence. Third sentence." = 6 words ≈ 9 tokens.
        // max_tokens=5 forces a split: each sentence (2 words ≈ 3 tokens) fits
        // alone. The chunker should therefore emit three sub-chunks, all flagged
        // is_oversized=true (the source paragraph was too large to fit) and all
        // referencing the same input index 0.
        let para = DocxElement::Paragraph {
            text: "First sentence. Second sentence. Third sentence.".into(),
            parent_heading: None,
            links: vec![],
        };
        let chunks = DocxRagChunker::new().with_max_tokens(5).chunk(&[para]);

        assert_eq!(chunks.len(), 3, "one chunk per sentence");
        assert_eq!(chunks[0].text, "First sentence.");
        assert_eq!(chunks[1].text, "Second sentence.");
        assert_eq!(chunks[2].text, "Third sentence.");
        for c in &chunks {
            assert!(
                c.is_oversized,
                "chunks born from a split paragraph must be flagged"
            );
            assert_eq!(c.paragraph_indices, vec![0]);
            assert_eq!(c.element_types, vec!["paragraph".to_string()]);
        }
    }

    #[test]
    fn second_heading_at_same_level_opens_new_chunk() {
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "A".into(),
            },
            DocxElement::Paragraph {
                text: "p1".into(),
                parent_heading: None,
                links: vec![],
            },
            DocxElement::Heading {
                level: 1,
                text: "B".into(),
            },
            DocxElement::Paragraph {
                text: "p2".into(),
                parent_heading: None,
                links: vec![],
            },
        ];
        let chunks = DocxRagChunker::new().chunk(&elements);
        assert_eq!(chunks.len(), 2);

        assert_eq!(chunks[0].text, "A\n\np1");
        assert_eq!(chunks[0].paragraph_indices, vec![0, 1]);
        assert_eq!(
            chunks[0].heading_context,
            vec![HeadingContext {
                level: 1,
                text: "A".into()
            }]
        );

        assert_eq!(chunks[1].text, "B\n\np2");
        assert_eq!(chunks[1].paragraph_indices, vec![2, 3]);
        assert_eq!(
            chunks[1].heading_context,
            vec![HeadingContext {
                level: 1,
                text: "B".into()
            }]
        );
    }

    #[test]
    fn inline_elements_exceeding_budget_split_across_multiple_chunks() {
        // Three paragraphs, 4 words each = 6 tokens each (4 * 1.5). With
        // max_tokens = 8, only one paragraph fits per chunk (two would be 12).
        // No heading, so before this cap existed all three merged into one
        // oversized chunk. Now: three chunks, each <= 8 tokens.
        let p = |t: &str| DocxElement::Paragraph {
            text: t.into(),
            parent_heading: None,
            links: vec![],
        };
        let elements = vec![
            p("one two three four"),
            p("five six seven eight"),
            p("nine ten eleven twelve"),
        ];
        let chunks = DocxRagChunker::new().with_max_tokens(8).chunk(&elements);

        assert_eq!(chunks.len(), 3, "each paragraph should be its own chunk");
        assert_eq!(chunks[0].text, "one two three four");
        assert_eq!(chunks[1].text, "five six seven eight");
        assert_eq!(chunks[2].text, "nine ten eleven twelve");
        for c in &chunks {
            assert!(c.token_estimate <= 8, "each chunk respects the cap");
            assert!(!c.is_oversized, "no single element exceeded the budget");
        }
        // Coverage invariant preserved.
        let all: Vec<usize> = chunks
            .iter()
            .flat_map(|c| c.paragraph_indices.iter().copied())
            .collect();
        assert_eq!(all, vec![0, 1, 2]);
    }

    #[test]
    fn heading_followed_by_paragraph_emits_single_chunk_with_heading_context() {
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "H".into(),
            },
            DocxElement::Paragraph {
                text: "body".into(),
                parent_heading: None,
                links: vec![],
            },
        ];
        let chunks = DocxRagChunker::new().chunk(&elements);
        assert_eq!(chunks.len(), 1, "single heading + paragraph → one chunk");
        let c = &chunks[0];
        assert_eq!(c.text, "H\n\nbody");
        assert_eq!(c.paragraph_indices, vec![0, 1]);
        assert_eq!(
            c.element_types,
            vec!["heading".to_string(), "paragraph".to_string()]
        );
        assert_eq!(
            c.heading_context,
            vec![HeadingContext {
                level: 1,
                text: "H".into(),
            }]
        );
        assert!(!c.is_oversized);
        assert!(c.token_estimate > 0);
    }

    #[test]
    fn table_breaks_the_running_buffer_into_its_own_chunk() {
        use crate::pipeline::element::{TableCell, TableRow};
        let para = |t: &str| DocxElement::Paragraph {
            text: t.into(),
            parent_heading: None,
            links: vec![],
        };
        let table = DocxElement::Table {
            rows: vec![TableRow {
                cells: vec![
                    TableCell {
                        text: "a".into(),
                        col_span: 1,
                        row_span: 1,
                    },
                    TableCell {
                        text: "b".into(),
                        col_span: 1,
                        row_span: 1,
                    },
                ],
            }],
        };
        // Small paragraphs that would otherwise merge; a table sits between them.
        let elements = vec![para("before text"), table, para("after text")];
        let chunks = DocxRagChunker::new().chunk(&elements);

        assert_eq!(chunks.len(), 3, "table must not merge with prose");
        assert_eq!(chunks[0].text, "before text");
        assert_eq!(chunks[0].element_types, vec!["paragraph".to_string()]);
        assert_eq!(chunks[1].text, "a | b");
        assert_eq!(chunks[1].element_types, vec!["table".to_string()]);
        assert_eq!(chunks[2].text, "after text");
    }

    #[test]
    fn same_type_only_policy_does_not_merge_paragraph_with_list_item() {
        use crate::numbering::ListType;
        use crate::pipeline::hybrid::MergePolicy;
        let elements = vec![
            DocxElement::Paragraph {
                text: "intro".into(),
                parent_heading: None,
                links: vec![],
            },
            DocxElement::ListItem {
                text: "first".into(),
                level: 0,
                list_type: ListType::Bullet,
                display_index: None,
            },
        ];

        // AnyInlineContent (default): both merge into one chunk.
        let merged = DocxRagChunker::new().chunk(&elements);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "intro\n\nfirst");

        // SameTypeOnly: paragraph and list_item stay separate.
        let split = DocxRagChunker::new()
            .with_merge_policy(MergePolicy::SameTypeOnly)
            .chunk(&elements);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].text, "intro");
        assert_eq!(split[0].element_types, vec!["paragraph".to_string()]);
        assert_eq!(split[1].text, "first");
        assert_eq!(split[1].element_types, vec!["list_item".to_string()]);
    }

    #[test]
    fn section_grouping_collapses_a_fitting_section_into_one_chunk() {
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "Intro".into(),
            },
            DocxElement::Paragraph {
                text: "alpha".into(),
                parent_heading: None,
                links: vec![],
            },
            DocxElement::Paragraph {
                text: "beta".into(),
                parent_heading: None,
                links: vec![],
            },
            DocxElement::Paragraph {
                text: "gamma".into(),
                parent_heading: None,
                links: vec![],
            },
        ];
        // Small tokens per paragraph; whole section fits an 800 budget.
        let chunks = DocxRagChunker::new()
            .with_section_grouping(true)
            .chunk(&elements);

        assert_eq!(chunks.len(), 1, "the whole heading section is one chunk");
        assert_eq!(chunks[0].text, "Intro\n\nalpha\n\nbeta\n\ngamma");
        assert_eq!(chunks[0].paragraph_indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn section_grouping_splits_and_restamps_heading_when_section_overflows() {
        let p = |t: &str| DocxElement::Paragraph {
            text: t.into(),
            parent_heading: None,
            links: vec![],
        };
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "Big".into(),
            },
            p("one two three four"),   // 6 tokens
            p("five six seven eight"), // 6 tokens
        ];
        // Heading (1 word => 2 tokens) + first para = 8; + second para = 14 > 10.
        let chunks = DocxRagChunker::new()
            .with_max_tokens(10)
            .with_section_grouping(true)
            .chunk(&elements);

        assert_eq!(
            chunks.len(),
            2,
            "overflowing section falls back to greedy pack"
        );
        // Both sub-chunks carry the section heading context.
        for c in &chunks {
            assert_eq!(
                c.heading_context,
                vec![HeadingContext {
                    level: 1,
                    text: "Big".into()
                }]
            );
        }
    }

    #[test]
    fn content_type_flags_reflect_table_and_list_presence() {
        use crate::numbering::ListType;
        let elements = vec![DocxElement::ListItem {
            text: "item".into(),
            level: 0,
            list_type: ListType::Bullet,
            display_index: None,
        }];
        let chunks = DocxRagChunker::new().chunk(&elements);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content_types.has_list);
        assert!(!chunks[0].content_types.has_table);
        assert!(!chunks[0].content_types.heading_only);

        // A lone heading chunk is heading_only.
        let just_heading = vec![DocxElement::Heading {
            level: 1,
            text: "H".into(),
        }];
        let hc = DocxRagChunker::new().chunk(&just_heading);
        assert!(hc[0].content_types.heading_only);
        assert!(!hc[0].content_types.has_list);
    }

    #[test]
    fn heading_path_and_full_text_are_derived_from_context() {
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "Chapter".into(),
            },
            DocxElement::Heading {
                level: 2,
                text: "Section".into(),
            },
            DocxElement::Paragraph {
                text: "body".into(),
                parent_heading: None,
                links: vec![],
            },
        ];
        let chunks = DocxRagChunker::new().chunk(&elements);
        // Two headings of different levels => the deepest chunk carries both.
        let last = chunks.last().unwrap();
        assert_eq!(
            last.heading_path,
            vec!["Chapter".to_string(), "Section".to_string()]
        );
        assert_eq!(last.full_text, "Chapter > Section\n\nSection\n\nbody");
    }

    #[test]
    fn full_text_without_heading_is_just_the_text() {
        let elements = vec![DocxElement::Paragraph {
            text: "orphan".into(),
            parent_heading: None,
            links: vec![],
        }];
        let chunks = DocxRagChunker::new().chunk(&elements);
        assert!(chunks[0].heading_path.is_empty());
        assert_eq!(chunks[0].full_text, "orphan");
    }
}
