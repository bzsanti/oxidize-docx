//! Packing algorithm for the RAG chunker, mirroring oxidize-pdf's
//! HybridChunker: heading-aware, element-type-aware, and size-aware
//! greedy packing over a `&[DocxElement]` stream.

use crate::pipeline::element::{DocxElement, HeadingContext, TableRow};
use crate::pipeline::rag::{ChunkAccumulator, RagChunk};

/// `word_count * 1.5` — crude, tokenizer-free approximation. Treat as an
/// upper bound; re-tokenize downstream if precision matters.
pub fn estimate_tokens(text: &str) -> usize {
    let words = text.split_whitespace().count();
    ((words as f64) * 1.5).ceil() as usize
}

pub(crate) fn table_to_text(rows: &[TableRow]) -> String {
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

pub(crate) fn split_sentences(text: &str) -> Vec<String> {
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

pub(crate) fn pack_sentences(sentences: Vec<String>, max_tokens: usize) -> Vec<String> {
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

/// Extracts the display text and element-type tag for a non-heading element.
/// Returns `None` for elements the chunker drops (headers/footers).
pub(crate) fn text_and_type(elem: &DocxElement) -> Option<(String, &'static str)> {
    match elem {
        DocxElement::Paragraph { text, .. } => Some((text.clone(), "paragraph")),
        DocxElement::ListItem { text, .. } => Some((text.clone(), "list_item")),
        DocxElement::Table { rows } => Some((table_to_text(rows), "table")),
        DocxElement::Footnote { id, text } => Some((format!("[{id}] {text}"), "footnote")),
        DocxElement::Endnote { id, text } => Some((format!("[endnote {id}] {text}"), "endnote")),
        DocxElement::Comment { id, author, text } => {
            Some((format!("[comment {id} by {author}] {text}"), "comment"))
        }
        DocxElement::Hyperlink { text, .. } => Some((text.clone(), "hyperlink")),
        // Page-level repeated content — dropped (duplicate noise across corpus).
        DocxElement::Header { .. } | DocxElement::Footer { .. } => None,
        DocxElement::Heading { .. } => None, // headings handled separately by the caller
    }
}

/// Core greedy packer. Walks elements in document order:
/// - a `Heading` flushes the buffer and seeds a new one (heading + following
///   inline content merge up to the cap);
/// - an inline element appends only if `buffer_tokens + elem_tokens <= max_tokens`,
///   else it flushes and starts a fresh buffer;
/// - an element whose own estimate already exceeds `max_tokens` is split at
///   sentence boundaries and each fragment emitted as an oversized chunk.
pub(crate) fn pack(elements: &[DocxElement], max_tokens: usize) -> Vec<RagChunk> {
    let mut out: Vec<RagChunk> = Vec::new();
    let mut heading_stack: Vec<HeadingContext> = Vec::new();
    let mut current = ChunkAccumulator::default();
    let mut current_tokens = 0usize;

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
            current_tokens = estimate_tokens(text);
            continue;
        }

        if let DocxElement::Table { rows } = elem {
            if !current.is_empty() {
                out.push(current.finalize(heading_stack.clone()));
                current = ChunkAccumulator::default();
                current_tokens = 0;
            }
            let text = table_to_text(rows);
            let elem_tokens = estimate_tokens(&text);
            if elem_tokens > max_tokens {
                // Non-splittable structural element: emit atomically, flagged.
                out.push(RagChunk::oversized_fragment(
                    text,
                    i,
                    "table",
                    heading_stack.clone(),
                    elem_tokens,
                ));
            } else {
                let mut acc = ChunkAccumulator::default();
                acc.push(i, text, "table");
                out.push(acc.finalize(heading_stack.clone()));
            }
            continue;
        }

        let Some((text, etype)) = text_and_type(elem) else {
            continue;
        };

        let elem_tokens = estimate_tokens(&text);

        if elem_tokens > max_tokens {
            if !current.is_empty() {
                out.push(current.finalize(heading_stack.clone()));
                current = ChunkAccumulator::default();
                current_tokens = 0;
            }
            for fragment in pack_sentences(split_sentences(&text), max_tokens) {
                let token_estimate = estimate_tokens(&fragment);
                out.push(RagChunk::oversized_fragment(
                    fragment,
                    i,
                    etype,
                    heading_stack.clone(),
                    token_estimate,
                ));
            }
            continue;
        }

        if !current.is_empty() && current_tokens + elem_tokens > max_tokens {
            out.push(current.finalize(heading_stack.clone()));
            current = ChunkAccumulator::default();
            current_tokens = 0;
        }

        current.push(i, text, etype);
        current_tokens += elem_tokens;
    }

    if !current.is_empty() {
        out.push(current.finalize(heading_stack));
    }
    out
}
