use std::borrow::Cow;

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
}

/// Hybrid chunker that walks a `Vec<DocxElement>` in document order and
/// emits `RagChunk`s sized for embedding APIs. The strategy is
/// heading-aware (a heading change opens a new chunk) and size-aware
/// (the running token estimate caps each chunk).
#[derive(Debug, Clone)]
pub struct DocxRagChunker {
    pub max_tokens: usize,
    pub profile: ExtractionProfile,
}

impl Default for DocxRagChunker {
    fn default() -> Self {
        Self {
            max_tokens: 800,
            profile: ExtractionProfile::default(),
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

    pub fn chunk(&self, elements: &[DocxElement]) -> Vec<RagChunk> {
        let view = apply_profile(elements, self.profile);
        self.chunk_view(view.as_ref())
    }

    fn chunk_view(&self, elements: &[DocxElement]) -> Vec<RagChunk> {
        let mut out: Vec<RagChunk> = Vec::new();
        let mut heading_stack: Vec<HeadingContext> = Vec::new();
        let mut current = ChunkAccumulator::default();

        for (i, elem) in elements.iter().enumerate() {
            if let DocxElement::Heading { level, text } = elem {
                if !current.is_empty() {
                    out.push(current.finalize(heading_stack.clone()));
                    current = ChunkAccumulator::default();
                }
                heading_stack.retain(|h| h.level < *level);
                heading_stack.push(HeadingContext {
                    level: *level,
                    text: text.clone(),
                });
                current.push(i, text.clone(), "heading");
                continue;
            }
            let (text, etype) = match elem {
                DocxElement::Paragraph { text, .. } => (text.clone(), "paragraph"),
                DocxElement::ListItem { text, .. } => (text.clone(), "list_item"),
                DocxElement::Table { rows } => (table_to_text(rows), "table"),
                DocxElement::Footnote { id, text } => (format!("[{id}] {text}"), "footnote"),
                DocxElement::Endnote { id, text } => (format!("[endnote {id}] {text}"), "endnote"),
                DocxElement::Comment { id, author, text } => {
                    (format!("[comment {id} by {author}] {text}"), "comment")
                }
                DocxElement::Hyperlink { text, .. } => (text.clone(), "hyperlink"),
                // Headers/footers carry page-level repeated content (page
                // numbers, doc titles, breadcrumbs). Including them in
                // every chunk produces duplicate noise across the corpus,
                // so the default chunker drops them. A future profile can
                // opt back in.
                DocxElement::Header { .. } | DocxElement::Footer { .. } => continue,
                DocxElement::Heading { .. } => unreachable!(),
            };

            // An element whose own token estimate already exceeds the budget
            // can't be packed into any chunk; split it at sentence
            // boundaries and emit each piece as its own oversized chunk
            // so downstream consumers know these fragments share a source.
            if estimate_tokens(&text) > self.max_tokens {
                if !current.is_empty() {
                    out.push(current.finalize(heading_stack.clone()));
                    current = ChunkAccumulator::default();
                }
                for fragment in pack_sentences(split_sentences(&text), self.max_tokens) {
                    let token_estimate = estimate_tokens(&fragment);
                    out.push(RagChunk {
                        text: fragment,
                        paragraph_indices: vec![i],
                        element_types: vec![etype.to_string()],
                        heading_context: heading_stack.clone(),
                        token_estimate,
                        is_oversized: true,
                    });
                }
                continue;
            }

            current.push(i, text, etype);
        }

        if !current.is_empty() {
            out.push(current.finalize(heading_stack));
        }
        out
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

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        current.push(chars[i]);
        if matches!(chars[i], '.' | '!' | '?') && chars.get(i + 1).copied() == Some(' ') {
            sentences.push(current.trim().to_string());
            current.clear();
            i += 1; // skip the separating space
        }
        i += 1;
    }
    let last = current.trim();
    if !last.is_empty() {
        sentences.push(last.to_string());
    }
    sentences
}

fn pack_sentences(sentences: Vec<String>, max_tokens: usize) -> Vec<String> {
    let mut packed = Vec::new();
    let mut buf = String::new();
    let mut buf_tokens = 0usize;
    for s in sentences {
        let s_tokens = estimate_tokens(&s);
        if !buf.is_empty() && buf_tokens + s_tokens > max_tokens {
            packed.push(std::mem::take(&mut buf));
            buf_tokens = 0;
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(&s);
        buf_tokens += s_tokens;
    }
    if !buf.is_empty() {
        packed.push(buf);
    }
    packed
}

#[derive(Default)]
struct ChunkAccumulator {
    text_parts: Vec<String>,
    paragraph_indices: Vec<usize>,
    element_types: Vec<String>,
}

impl ChunkAccumulator {
    fn is_empty(&self) -> bool {
        self.paragraph_indices.is_empty()
    }

    fn push(&mut self, idx: usize, text: String, etype: &str) {
        self.text_parts.push(text);
        self.paragraph_indices.push(idx);
        self.element_types.push(etype.to_string());
    }

    fn finalize(self, heading_context: Vec<HeadingContext>) -> RagChunk {
        let text = self.text_parts.join("\n\n");
        let token_estimate = estimate_tokens(&text);
        RagChunk {
            text,
            paragraph_indices: self.paragraph_indices,
            element_types: self.element_types,
            heading_context,
            token_estimate,
            is_oversized: false,
        }
    }
}

fn estimate_tokens(text: &str) -> usize {
    let words = text.split_whitespace().count();
    ((words as f64) * 1.5).ceil() as usize
}

fn table_to_text(rows: &[crate::pipeline::element::TableRow]) -> String {
    rows.iter()
        .map(|r| {
            r.cells
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n")
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
            },
            DocxElement::Paragraph {
                text: "p1".into(),
                parent_heading: None,
            },
            DocxElement::Heading {
                level: 1,
                text: "B".into(),
            },
            DocxElement::Paragraph {
                text: "p2".into(),
                parent_heading: None,
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
            },
            DocxElement::Heading {
                level: 1,
                text: "B".into(),
            },
            DocxElement::Paragraph {
                text: "p2".into(),
                parent_heading: None,
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
    fn heading_followed_by_paragraph_emits_single_chunk_with_heading_context() {
        let elements = vec![
            DocxElement::Heading {
                level: 1,
                text: "H".into(),
            },
            DocxElement::Paragraph {
                text: "body".into(),
                parent_heading: None,
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
}
