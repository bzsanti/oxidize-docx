# Align RAG Chunking with oxidize-pdf — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring oxidize-docx's RAG chunker to behavioral + metadata parity with oxidize-pdf's HybridChunker (inter-element size cap, merge policy, structural breaks, section-grouping, rich chunk metadata), keeping docx's ×1.5 token model and remapping profiles to pdf's names.

**Architecture:** The packing algorithm moves into a new `pipeline/hybrid.rs` consumed by `DocxRagChunker`; a new `pipeline/chunk_metadata.rs` holds metadata helpers (`ContentTypeFlags`, `chunk_id`, `link_chunks`). `RagChunk` gains flat metadata fields. `ExtractionProfile` is remapped to 5 pdf-mirrored variants.

**Tech Stack:** Rust, `quick-xml`, `sha2` (new, pure Rust), cargo test (TDD).

## Global Constraints

- Rust edition workspace, `rust-version = "1.77"`.
- Pure Rust only — no C/FFI deps. `sha2` is pure Rust (OK).
- `cargo clippy --all-targets -- -D warnings` must stay clean (warnings = errors).
- `cargo fmt --check` must stay clean.
- No smoke tests: assert real chunk content, not counts/sizes alone.
- TDD strict: each GREEN implements only what the RED test demands.
- Token estimate stays `word_count * 1.5` (`estimate_tokens`), unchanged.
- `max_tokens` default stays `800`.
- New public types must be re-exported through `pipeline/mod.rs`.
- Add every new file to git.

---

## File Structure

- Modify `Cargo.toml` — workspace version `0.1.0`→`0.2.0`; add `sha2` to `[workspace.dependencies]`.
- Modify `oxidize-docx-core/Cargo.toml` — add `sha2 = { workspace = true }`.
- Create `oxidize-docx-core/src/pipeline/hybrid.rs` — `MergePolicy`, element classification, greedy packing with cap, oversized split, section-grouping. Houses the moved helpers `estimate_tokens`, `split_sentences`, `pack_sentences`, `table_to_text`.
- Create `oxidize-docx-core/src/pipeline/chunk_metadata.rs` — `ContentTypeFlags`, `content_type_flags()`, `compute_chunk_id()`, `link_chunks()`, `heading_path_from()`.
- Modify `oxidize-docx-core/src/pipeline/rag.rs` — `RagChunk` new fields; `DocxRagChunker` new config; delegate to `hybrid`.
- Modify `oxidize-docx-core/src/pipeline/profile.rs` — remap enum to `Standard`/`Rag`/`Academic`/`Dense`/`Technical`.
- Modify `oxidize-docx-core/src/pipeline/mod.rs` — declare `hybrid`, `chunk_metadata`; re-export `MergePolicy`, `ContentTypeFlags`.
- Modify `oxidize-docx-core/src/document.rs` — doc comment on default; keep `rag_chunks*` API.
- Modify `CLAUDE.md` — correct "misma implementación que oxidize-pdf" claim.
- Modify `docs/ROADMAP.md` — record alignment; close Fase 4 inter-element chunking pendiente.

**Note on `RagChunk` equality tests:** existing tests build chunks via the chunker and assert individual fields (`c.text`, `c.paragraph_indices`, …) or compare whole `Vec<RagChunk>` produced by the same chunker. Adding fields is safe: both sides of any `==` flow through identical code, and field-level asserts are unaffected.

---

## Task 1: Version bump to 0.2.0

**Files:**
- Modify: `Cargo.toml:6`

**Interfaces:**
- Produces: workspace at version `0.2.0`.

- [ ] **Step 1: Bump the workspace version**

In `Cargo.toml`, change line 6:

```toml
version = "0.2.0"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles; crate reports `oxidize-docx v0.2.0`.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: bump version to 0.2.0 for RAG alignment cycle"
```

---

## Task 2: Add `sha2` dependency

**Files:**
- Modify: `Cargo.toml:13-22` (workspace.dependencies)
- Modify: `oxidize-docx-core/Cargo.toml:20-26` (dependencies)

**Interfaces:**
- Produces: `sha2::{Sha256, Digest}` available in the core crate.

- [ ] **Step 1: Add sha2 to workspace dependencies**

In `Cargo.toml`, under `[workspace.dependencies]`, add:

```toml
sha2 = "0.10"
```

- [ ] **Step 2: Add sha2 to the core crate**

In `oxidize-docx-core/Cargo.toml`, under `[dependencies]`, add:

```toml
sha2 = { workspace = true }
```

- [ ] **Step 3: Verify it resolves and compiles**

Run: `cargo check`
Expected: `sha2 v0.10.x` downloaded/compiled; no errors.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml oxidize-docx-core/Cargo.toml Cargo.lock
git commit -m "chore: add sha2 dependency for deterministic chunk_id"
```

---

## Task 3: Extract packing into `hybrid.rs` + inter-element size cap

This is the core gap. Today a chunk grows unbounded until a heading. After this task, a running buffer flushes when the next inline element would exceed `max_tokens`.

**Files:**
- Create: `oxidize-docx-core/src/pipeline/hybrid.rs`
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Modify: `oxidize-docx-core/src/pipeline/mod.rs`
- Test: in `oxidize-docx-core/src/pipeline/rag.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces (in `hybrid.rs`):
  - `pub fn estimate_tokens(text: &str) -> usize`
  - `pub(crate) fn split_sentences(text: &str) -> Vec<String>`
  - `pub(crate) fn pack_sentences(sentences: Vec<String>, max_tokens: usize) -> Vec<String>`
  - `pub(crate) fn table_to_text(rows: &[crate::pipeline::element::TableRow]) -> String`
- Consumes: `DocxElement`, `HeadingContext`, `RagChunk`, `ChunkAccumulator`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `oxidize-docx-core/src/pipeline/rag.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oxidize-docx-core inline_elements_exceeding_budget -- --nocapture`
Expected: FAIL — one chunk of all three paragraphs (old behavior has no cap).

- [ ] **Step 3: Create `hybrid.rs` with helpers and the capped packer**

Create `oxidize-docx-core/src/pipeline/hybrid.rs`:

```rust
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
                current_tokens = 0;
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
```

- [ ] **Step 4: Expose `ChunkAccumulator` to `hybrid` and add `RagChunk::oversized_fragment`**

In `oxidize-docx-core/src/pipeline/rag.rs`, change `struct ChunkAccumulator` visibility and its impl block to `pub(crate)`, and its fields/methods to `pub(crate)`:

```rust
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

    pub(crate) fn finalize(self, heading_context: Vec<HeadingContext>) -> RagChunk {
        let text = self.text_parts.join("\n\n");
        let token_estimate = crate::pipeline::hybrid::estimate_tokens(&text);
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

impl RagChunk {
    pub(crate) fn oversized_fragment(
        text: String,
        idx: usize,
        etype: &str,
        heading_context: Vec<HeadingContext>,
        token_estimate: usize,
    ) -> Self {
        RagChunk {
            text,
            paragraph_indices: vec![idx],
            element_types: vec![etype.to_string()],
            heading_context,
            token_estimate,
            is_oversized: true,
        }
    }
}
```

- [ ] **Step 5: Replace `chunk_view` body with a call to `hybrid::pack`**

In `oxidize-docx-core/src/pipeline/rag.rs`, replace the entire `fn chunk_view` implementation with:

```rust
    fn chunk_view(&self, elements: &[DocxElement]) -> Vec<RagChunk> {
        crate::pipeline::hybrid::pack(elements, self.max_tokens)
    }
```

Then delete from `rag.rs` the now-moved free functions `estimate_tokens`, `split_sentences`, `pack_sentences`, and `table_to_text` (they live in `hybrid.rs`). Update `academic_inline`/any caller that referenced `estimate_tokens` to use `crate::pipeline::hybrid::estimate_tokens`.

- [ ] **Step 6: Declare the module**

In `oxidize-docx-core/src/pipeline/mod.rs`, add after the existing `mod` lines:

```rust
pub(crate) mod hybrid;
```

- [ ] **Step 7: Run the new test and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: the new test PASSES; all previously-green tests still pass (the oversized-split test still holds because the split path is preserved).

- [ ] **Step 8: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add oxidize-docx-core/src/pipeline/hybrid.rs oxidize-docx-core/src/pipeline/rag.rs oxidize-docx-core/src/pipeline/mod.rs
git commit -m "feat(rag): enforce inter-element size cap via hybrid packer"
```

---

## Task 4: Structural break for tables

Tables must not be swept into a running prose buffer; a `Table` flushes the buffer and becomes its own chunk (matching pdf's structural-break rule). Headings already flush.

**Files:**
- Modify: `oxidize-docx-core/src/pipeline/hybrid.rs`
- Test: `oxidize-docx-core/src/pipeline/rag.rs`

**Interfaces:**
- Consumes: `hybrid::pack`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `rag.rs`:

```rust
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
                TableCell { text: "a".into(), col_span: 1, row_span: 1 },
                TableCell { text: "b".into(), col_span: 1, row_span: 1 },
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oxidize-docx-core table_breaks_the_running_buffer -- --nocapture`
Expected: FAIL — currently the table merges with the surrounding paragraphs into one chunk.

- [ ] **Step 3: Add the structural-break branch in `pack`**

In `hybrid.rs`, inside `pack`, immediately after the heading `if let` block and before `let Some((text, etype)) = ...`, insert:

```rust
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
```

- [ ] **Step 4: Run the new test and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: new test PASSES; all prior tests pass. (The prior integration/markdown tests that place tables next to other elements still hold because table text is unchanged.)

- [ ] **Step 5: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add oxidize-docx-core/src/pipeline/hybrid.rs oxidize-docx-core/src/pipeline/rag.rs
git commit -m "feat(rag): tables break the buffer as structural elements"
```

---

## Task 5: MergePolicy (AnyInlineContent | SameTypeOnly)

**Files:**
- Modify: `oxidize-docx-core/src/pipeline/hybrid.rs`
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Modify: `oxidize-docx-core/src/pipeline/mod.rs`
- Test: `oxidize-docx-core/src/pipeline/rag.rs`

**Interfaces:**
- Produces (in `hybrid.rs`): `pub enum MergePolicy { AnyInlineContent, SameTypeOnly }` (derives `Debug, Clone, Copy, PartialEq, Eq, Default`; `AnyInlineContent` is `#[default]`).
- Produces (in `rag.rs`): `DocxRagChunker.merge_policy: MergePolicy` field + `with_merge_policy(self, MergePolicy) -> Self`.
- `hybrid::pack` gains a `policy: MergePolicy` parameter: `pack(elements: &[DocxElement], max_tokens: usize, policy: MergePolicy)`.

- [ ] **Step 1: Write the failing test**

Add to `rag.rs` tests:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oxidize-docx-core same_type_only_policy -- --nocapture`
Expected: FAIL to compile — `MergePolicy` and `with_merge_policy` don't exist.

- [ ] **Step 3: Define `MergePolicy` and thread it through `pack`**

In `hybrid.rs`, add near the top (after imports):

```rust
/// Governs whether adjacent inline elements of different types may share a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergePolicy {
    /// Any two inline elements may merge (subject to the size cap).
    #[default]
    AnyInlineContent,
    /// Only elements with the same `element_type` may merge.
    SameTypeOnly,
}
```

Change the `pack` signature to `pub(crate) fn pack(elements: &[DocxElement], max_tokens: usize, policy: MergePolicy) -> Vec<RagChunk>`.

In `pack`, replace the inline append guard:

```rust
        if !current.is_empty() && current_tokens + elem_tokens > max_tokens {
```

with a guard that also honors the policy:

```rust
        let type_blocks_merge = policy == MergePolicy::SameTypeOnly
            && current.last_type().map(|t| t != etype).unwrap_or(false);
        if !current.is_empty() && (current_tokens + elem_tokens > max_tokens || type_blocks_merge) {
```

- [ ] **Step 4: Add `last_type` to `ChunkAccumulator`**

In `rag.rs`, add to the `ChunkAccumulator` impl:

```rust
    pub(crate) fn last_type(&self) -> Option<&str> {
        self.element_types.last().map(|s| s.as_str())
    }
```

- [ ] **Step 5: Add the config field and builder, update `chunk_view`**

In `rag.rs`, add `pub merge_policy: crate::pipeline::hybrid::MergePolicy` to `DocxRagChunker`, default it in `Default::default()` to `MergePolicy::default()`, add:

```rust
    pub fn with_merge_policy(mut self, policy: crate::pipeline::hybrid::MergePolicy) -> Self {
        self.merge_policy = policy;
        self
    }
```

Update `chunk_view` to pass the policy:

```rust
    fn chunk_view(&self, elements: &[DocxElement]) -> Vec<RagChunk> {
        crate::pipeline::hybrid::pack(elements, self.max_tokens, self.merge_policy)
    }
```

- [ ] **Step 6: Re-export `MergePolicy`**

In `pipeline/mod.rs`, add:

```rust
pub use hybrid::MergePolicy;
```

- [ ] **Step 7: Run the new test and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: new test PASSES; all prior tests pass.

- [ ] **Step 8: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add oxidize-docx-core/src/pipeline/hybrid.rs oxidize-docx-core/src/pipeline/rag.rs oxidize-docx-core/src/pipeline/mod.rs
git commit -m "feat(rag): add MergePolicy (AnyInlineContent | SameTypeOnly)"
```

---

## Task 6: Section-grouping (`chunk_with_graph` analog)

Groups all elements under a heading into one chunk when the whole section fits `max_tokens`; otherwise delegates to `pack` and stamps the section heading on every sub-chunk. Toggled by a config flag (later enabled by the `Rag` profile).

**Files:**
- Modify: `oxidize-docx-core/src/pipeline/hybrid.rs`
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Test: `oxidize-docx-core/src/pipeline/rag.rs`

**Interfaces:**
- Produces (in `hybrid.rs`): `pub(crate) fn pack_grouped(elements: &[DocxElement], max_tokens: usize, policy: MergePolicy) -> Vec<RagChunk>`.
- Produces (in `rag.rs`): `DocxRagChunker.group_sections: bool` + `with_section_grouping(self, bool) -> Self`.

- [ ] **Step 1: Write the failing test**

Add to `rag.rs` tests:

```rust
#[test]
fn section_grouping_collapses_a_fitting_section_into_one_chunk() {
    let elements = vec![
        DocxElement::Heading { level: 1, text: "Intro".into() },
        DocxElement::Paragraph { text: "alpha".into(), parent_heading: None, links: vec![] },
        DocxElement::Paragraph { text: "beta".into(), parent_heading: None, links: vec![] },
        DocxElement::Paragraph { text: "gamma".into(), parent_heading: None, links: vec![] },
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
    let p = |t: &str| DocxElement::Paragraph { text: t.into(), parent_heading: None, links: vec![] };
    let elements = vec![
        DocxElement::Heading { level: 1, text: "Big".into() },
        p("one two three four"),   // 6 tokens
        p("five six seven eight"), // 6 tokens
    ];
    // Heading (1 word => 2 tokens) + first para = 8; + second para = 14 > 10.
    let chunks = DocxRagChunker::new()
        .with_max_tokens(10)
        .with_section_grouping(true)
        .chunk(&elements);

    assert_eq!(chunks.len(), 2, "overflowing section falls back to greedy pack");
    // Both sub-chunks carry the section heading context.
    for c in &chunks {
        assert_eq!(
            c.heading_context,
            vec![HeadingContext { level: 1, text: "Big".into() }]
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oxidize-docx-core section_grouping -- --nocapture`
Expected: FAIL to compile — `with_section_grouping` doesn't exist.

- [ ] **Step 3: Implement `pack_grouped`**

In `hybrid.rs`, add:

```rust
/// Section-aware packing. A `Heading` opens a section spanning every element
/// up to (but not including) the next heading of equal-or-shallower level. If
/// the whole section fits `max_tokens`, it becomes one chunk; otherwise the
/// section is packed greedily and each resulting chunk keeps the section's
/// heading context. Preamble before the first heading is packed greedily.
pub(crate) fn pack_grouped(
    elements: &[DocxElement],
    max_tokens: usize,
    policy: MergePolicy,
) -> Vec<RagChunk> {
    let mut out: Vec<RagChunk> = Vec::new();
    let mut i = 0usize;
    while i < elements.len() {
        let DocxElement::Heading { level, .. } = &elements[i] else {
            // Preamble run until the first heading.
            let start = i;
            while i < elements.len() && !matches!(elements[i], DocxElement::Heading { .. }) {
                i += 1;
            }
            out.extend(reindex(pack(&elements[start..i], max_tokens, policy), start));
            continue;
        };
        let section_level = *level;
        let start = i;
        i += 1;
        while i < elements.len() {
            if let DocxElement::Heading { level: l, .. } = &elements[i] {
                if *l <= section_level {
                    break;
                }
            }
            i += 1;
        }
        let section = &elements[start..i];
        let section_text_tokens: usize = section
            .iter()
            .filter_map(|e| section_element_tokens(e))
            .sum();
        let mut packed = if section_text_tokens <= max_tokens {
            single_chunk(section)
        } else {
            pack(section, max_tokens, policy)
        };
        for c in &mut packed {
            for idx in &mut c.paragraph_indices {
                *idx += start;
            }
        }
        out.extend(packed);
    }
    out
}

fn section_element_tokens(e: &DocxElement) -> Option<usize> {
    match e {
        DocxElement::Heading { text, .. } => Some(estimate_tokens(text)),
        other => text_and_type(other).map(|(t, _)| estimate_tokens(&t)),
    }
}

/// Packs an entire section (heading + body) into exactly one chunk. Assumes the
/// caller verified it fits `max_tokens`. Indices are section-relative and are
/// rebased by the caller.
fn single_chunk(section: &[DocxElement]) -> Vec<RagChunk> {
    let mut acc = ChunkAccumulator::default();
    let mut heading_ctx: Vec<HeadingContext> = Vec::new();
    for (rel, e) in section.iter().enumerate() {
        if let DocxElement::Heading { level, text } = e {
            heading_ctx.retain(|h| h.level < *level);
            heading_ctx.push(HeadingContext { level: *level, text: text.clone() });
            acc.push(rel, text.clone(), "heading");
            continue;
        }
        if let Some((text, etype)) = text_and_type(e) {
            acc.push(rel, text, etype);
        }
    }
    if acc.is_empty() {
        return Vec::new();
    }
    vec![acc.finalize(heading_ctx)]
}

/// Rebases section-relative `paragraph_indices` by `offset`.
fn reindex(mut chunks: Vec<RagChunk>, offset: usize) -> Vec<RagChunk> {
    for c in &mut chunks {
        for idx in &mut c.paragraph_indices {
            *idx += offset;
        }
    }
    chunks
}
```

Note: `pack` produces section-relative indices when called on a slice, so `pack_grouped` rebases them (`+= start` / `reindex`). This keeps the global coverage invariant.

- [ ] **Step 4: Add the config flag and route `chunk_view`**

In `rag.rs`, add `pub group_sections: bool` to `DocxRagChunker` (default `false`), add:

```rust
    pub fn with_section_grouping(mut self, on: bool) -> Self {
        self.group_sections = on;
        self
    }
```

Update `chunk_view`:

```rust
    fn chunk_view(&self, elements: &[DocxElement]) -> Vec<RagChunk> {
        if self.group_sections {
            crate::pipeline::hybrid::pack_grouped(elements, self.max_tokens, self.merge_policy)
        } else {
            crate::pipeline::hybrid::pack(elements, self.max_tokens, self.merge_policy)
        }
    }
```

- [ ] **Step 5: Run the new tests and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: both new tests PASS; all prior tests pass.

- [ ] **Step 6: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add oxidize-docx-core/src/pipeline/hybrid.rs oxidize-docx-core/src/pipeline/rag.rs
git commit -m "feat(rag): section-grouping packer (chunk_with_graph analog)"
```

---

## Task 7: `ContentTypeFlags` metadata + `content_types` field

**Files:**
- Create: `oxidize-docx-core/src/pipeline/chunk_metadata.rs`
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Modify: `oxidize-docx-core/src/pipeline/mod.rs`
- Test: `oxidize-docx-core/src/pipeline/rag.rs`

**Interfaces:**
- Produces (in `chunk_metadata.rs`):
  - `pub struct ContentTypeFlags { pub has_table: bool, pub has_list: bool, pub heading_only: bool }` (derives `Debug, Clone, Copy, PartialEq, Eq, Default`).
  - `pub(crate) fn content_type_flags(element_types: &[String]) -> ContentTypeFlags`.
- Produces (in `rag.rs`): `RagChunk.content_types: ContentTypeFlags`, populated in `finalize` and `oversized_fragment`.

- [ ] **Step 1: Write the failing test**

Add to `rag.rs` tests:

```rust
#[test]
fn content_type_flags_reflect_table_and_list_presence() {
    use crate::numbering::ListType;
    let elements = vec![
        DocxElement::ListItem {
            text: "item".into(),
            level: 0,
            list_type: ListType::Bullet,
            display_index: None,
        },
    ];
    let chunks = DocxRagChunker::new().chunk(&elements);
    assert_eq!(chunks.len(), 1);
    assert!(chunks[0].content_types.has_list);
    assert!(!chunks[0].content_types.has_table);
    assert!(!chunks[0].content_types.heading_only);

    // A lone heading chunk is heading_only.
    let just_heading = vec![DocxElement::Heading { level: 1, text: "H".into() }];
    let hc = DocxRagChunker::new().chunk(&just_heading);
    assert!(hc[0].content_types.heading_only);
    assert!(!hc[0].content_types.has_list);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oxidize-docx-core content_type_flags -- --nocapture`
Expected: FAIL to compile — `content_types` field doesn't exist.

- [ ] **Step 3: Create `chunk_metadata.rs`**

Create `oxidize-docx-core/src/pipeline/chunk_metadata.rs`:

```rust
//! Metadata helpers for RAG chunks: content-type flags, deterministic
//! chunk ids, and prev/next linking. Mirrors the applicable subset of
//! oxidize-pdf's `pipeline/chunk_metadata.rs` (no font/coordinate/page
//! metadata — docx has no source for those).

/// Coarse flags describing what an emitted chunk contains, so downstream
/// rerankers can route (e.g. tables to a table-QA path) without re-parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContentTypeFlags {
    pub has_table: bool,
    pub has_list: bool,
    /// True when the chunk holds only a heading and no body content.
    pub heading_only: bool,
}

/// Derives content flags from a chunk's `element_types` tags.
pub(crate) fn content_type_flags(element_types: &[String]) -> ContentTypeFlags {
    let has_table = element_types.iter().any(|t| t == "table");
    let has_list = element_types.iter().any(|t| t == "list_item");
    let heading_only =
        !element_types.is_empty() && element_types.iter().all(|t| t == "heading");
    ContentTypeFlags {
        has_table,
        has_list,
        heading_only,
    }
}
```

- [ ] **Step 4: Add the field and populate it**

In `rag.rs`:
- Add `use crate::pipeline::chunk_metadata::{content_type_flags, ContentTypeFlags};` at the top.
- Add `pub content_types: ContentTypeFlags,` to `struct RagChunk`.
- In `ChunkAccumulator::finalize`, compute and set it:

```rust
    pub(crate) fn finalize(self, heading_context: Vec<HeadingContext>) -> RagChunk {
        let text = self.text_parts.join("\n\n");
        let token_estimate = crate::pipeline::hybrid::estimate_tokens(&text);
        let content_types = content_type_flags(&self.element_types);
        RagChunk {
            text,
            paragraph_indices: self.paragraph_indices,
            element_types: self.element_types,
            heading_context,
            token_estimate,
            is_oversized: false,
            content_types,
        }
    }
```

- In `RagChunk::oversized_fragment`, set `content_types: content_type_flags(&[etype.to_string()])` (compute before moving `etype` into the vec).

- [ ] **Step 5: Declare and re-export**

In `pipeline/mod.rs`, add:

```rust
pub(crate) mod chunk_metadata;
```

and:

```rust
pub use chunk_metadata::ContentTypeFlags;
```

- [ ] **Step 6: Run the new test and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: new test PASSES; all prior tests pass (existing field-level asserts unaffected).

- [ ] **Step 7: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add oxidize-docx-core/src/pipeline/chunk_metadata.rs oxidize-docx-core/src/pipeline/rag.rs oxidize-docx-core/src/pipeline/mod.rs
git commit -m "feat(rag): add ContentTypeFlags metadata to chunks"
```

---

## Task 8: `heading_path` + `full_text` fields

**Files:**
- Modify: `oxidize-docx-core/src/pipeline/chunk_metadata.rs`
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Test: `oxidize-docx-core/src/pipeline/rag.rs`

**Interfaces:**
- Produces (in `chunk_metadata.rs`): `pub(crate) fn heading_path_from(ctx: &[crate::pipeline::element::HeadingContext]) -> Vec<String>`.
- Produces (in `rag.rs`): `RagChunk.heading_path: Vec<String>`, `RagChunk.full_text: String` (`heading_path.join(" > ")` + `"\n\n"` + `text`, or just `text` when no heading).

- [ ] **Step 1: Write the failing test**

Add to `rag.rs` tests:

```rust
#[test]
fn heading_path_and_full_text_are_derived_from_context() {
    let elements = vec![
        DocxElement::Heading { level: 1, text: "Chapter".into() },
        DocxElement::Heading { level: 2, text: "Section".into() },
        DocxElement::Paragraph { text: "body".into(), parent_heading: None, links: vec![] },
    ];
    let chunks = DocxRagChunker::new().chunk(&elements);
    // Two headings of different levels => the deepest chunk carries both.
    let last = chunks.last().unwrap();
    assert_eq!(last.heading_path, vec!["Chapter".to_string(), "Section".to_string()]);
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oxidize-docx-core heading_path -- --nocapture`
Expected: FAIL to compile — fields don't exist.

- [ ] **Step 3: Add `heading_path_from`**

In `chunk_metadata.rs`, add:

```rust
use crate::pipeline::element::HeadingContext;

/// Flattens a heading context stack (root→leaf) to its text breadcrumb.
pub(crate) fn heading_path_from(ctx: &[HeadingContext]) -> Vec<String> {
    ctx.iter().map(|h| h.text.clone()).collect()
}
```

- [ ] **Step 4: Add fields and populate them**

In `rag.rs`:
- Add `use crate::pipeline::chunk_metadata::heading_path_from;`.
- Add `pub heading_path: Vec<String>,` and `pub full_text: String,` to `struct RagChunk`.
- Add a private helper in `rag.rs`:

```rust
fn build_full_text(heading_path: &[String], text: &str) -> String {
    if heading_path.is_empty() {
        text.to_string()
    } else {
        format!("{}\n\n{}", heading_path.join(" > "), text)
    }
}
```

- In `finalize`, after computing `text` and before constructing the chunk:

```rust
        let heading_path = heading_path_from(&heading_context);
        let full_text = build_full_text(&heading_path, &text);
```

and add `heading_path, full_text,` to the returned `RagChunk`.
- In `oversized_fragment`, compute `let heading_path = heading_path_from(&heading_context); let full_text = build_full_text(&heading_path, &text);` and add both fields.

- [ ] **Step 5: Run the new tests and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: new tests PASS; all prior tests pass.

- [ ] **Step 6: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add oxidize-docx-core/src/pipeline/chunk_metadata.rs oxidize-docx-core/src/pipeline/rag.rs
git commit -m "feat(rag): add heading_path and full_text to chunks"
```

---

## Task 9: `chunk_index` + deterministic `chunk_id` (sha2)

**Files:**
- Modify: `oxidize-docx-core/src/pipeline/chunk_metadata.rs`
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Test: `oxidize-docx-core/src/pipeline/rag.rs`

**Interfaces:**
- Produces (in `chunk_metadata.rs`): `pub(crate) fn compute_chunk_id(full_text: &str, index: usize) -> String` → `"{doc_id}:{index}"` where `doc_id` = first 8 bytes of `SHA-256(full_text)` as 16 lowercase hex chars.
- Produces (in `rag.rs`): `RagChunk.chunk_index: usize`, `RagChunk.chunk_id: String`, assigned in a post-pass over the emitted `Vec` inside `DocxRagChunker::chunk`.

- [ ] **Step 1: Write the failing test**

Add to `rag.rs` tests:

```rust
#[test]
fn chunk_ids_are_deterministic_and_indexed() {
    let elements = vec![
        DocxElement::Heading { level: 1, text: "A".into() },
        DocxElement::Paragraph { text: "body".into(), parent_heading: None, links: vec![] },
        DocxElement::Heading { level: 1, text: "B".into() },
        DocxElement::Paragraph { text: "more".into(), parent_heading: None, links: vec![] },
    ];
    let first = DocxRagChunker::new().chunk(&elements);
    let second = DocxRagChunker::new().chunk(&elements);

    // chunk_index is 0-based positional.
    assert_eq!(first[0].chunk_index, 0);
    assert_eq!(first[1].chunk_index, 1);

    // chunk_id shape: 16 hex chars, ':', index.
    assert!(first[0].chunk_id.ends_with(":0"));
    let doc_id = first[0].chunk_id.split(':').next().unwrap();
    assert_eq!(doc_id.len(), 16);
    assert!(doc_id.chars().all(|c| c.is_ascii_hexdigit()));

    // Determinism: same input => same ids.
    assert_eq!(first[0].chunk_id, second[0].chunk_id);
    // Distinct content => distinct doc_id prefixes.
    assert_ne!(
        first[0].chunk_id.split(':').next().unwrap(),
        first[1].chunk_id.split(':').next().unwrap()
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oxidize-docx-core chunk_ids_are_deterministic -- --nocapture`
Expected: FAIL to compile — `chunk_index`/`chunk_id` don't exist.

- [ ] **Step 3: Add `compute_chunk_id`**

In `chunk_metadata.rs`, add:

```rust
use sha2::{Digest, Sha256};

/// Deterministic chunk id `"{doc_id}:{index}"`. `doc_id` is the first 8 bytes
/// of SHA-256 over the chunk's `full_text`, hex-encoded (16 chars). Same
/// content always yields the same id — stable across runs and machines.
pub(crate) fn compute_chunk_id(full_text: &str, index: usize) -> String {
    let digest = Sha256::digest(full_text.as_bytes());
    let mut doc_id = String::with_capacity(16);
    for byte in &digest[..8] {
        doc_id.push_str(&format!("{byte:02x}"));
    }
    format!("{doc_id}:{index}")
}
```

- [ ] **Step 4: Add fields and assign them in a post-pass**

In `rag.rs`:
- Add `use crate::pipeline::chunk_metadata::compute_chunk_id;`.
- Add `pub chunk_index: usize,` and `pub chunk_id: String,` to `struct RagChunk`.
- In `finalize` and `oversized_fragment`, initialize both to placeholders: `chunk_index: 0, chunk_id: String::new()` (they're stamped in the post-pass).
- In `DocxRagChunker::chunk`, after `let mut chunks = self.chunk_view(view.as_ref());` (change `chunk_view` result binding to `mut`), add before returning:

```rust
        for (i, c) in chunks.iter_mut().enumerate() {
            c.chunk_index = i;
            c.chunk_id = compute_chunk_id(&c.full_text, i);
        }
        chunks
```

Ensure `chunk` returns `chunks`.

- [ ] **Step 5: Run the new test and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: new test PASSES; all prior tests pass.

- [ ] **Step 6: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add oxidize-docx-core/src/pipeline/chunk_metadata.rs oxidize-docx-core/src/pipeline/rag.rs
git commit -m "feat(rag): deterministic chunk_id and chunk_index"
```

---

## Task 10: `prev_chunk_id` / `next_chunk_id` linking

**Files:**
- Modify: `oxidize-docx-core/src/pipeline/chunk_metadata.rs`
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Test: `oxidize-docx-core/src/pipeline/rag.rs`

**Interfaces:**
- Produces (in `chunk_metadata.rs`): `pub(crate) fn link_chunks(chunks: &mut [RagChunk])` — sets each chunk's `prev_chunk_id`/`next_chunk_id` from neighbors' `chunk_id`.
- Produces (in `rag.rs`): `RagChunk.prev_chunk_id: Option<String>`, `RagChunk.next_chunk_id: Option<String>`.

- [ ] **Step 1: Write the failing test**

Add to `rag.rs` tests:

```rust
#[test]
fn chunks_are_linked_into_a_doubly_linked_chain() {
    let elements = vec![
        DocxElement::Heading { level: 1, text: "A".into() },
        DocxElement::Paragraph { text: "one".into(), parent_heading: None, links: vec![] },
        DocxElement::Heading { level: 1, text: "B".into() },
        DocxElement::Paragraph { text: "two".into(), parent_heading: None, links: vec![] },
    ];
    let chunks = DocxRagChunker::new().chunk(&elements);
    assert_eq!(chunks.len(), 2);

    assert_eq!(chunks[0].prev_chunk_id, None);
    assert_eq!(chunks[0].next_chunk_id, Some(chunks[1].chunk_id.clone()));
    assert_eq!(chunks[1].prev_chunk_id, Some(chunks[0].chunk_id.clone()));
    assert_eq!(chunks[1].next_chunk_id, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p oxidize-docx-core doubly_linked_chain -- --nocapture`
Expected: FAIL to compile — fields don't exist.

- [ ] **Step 3: Add `link_chunks`**

In `chunk_metadata.rs`, add:

```rust
use crate::pipeline::rag::RagChunk;

/// Links chunks into a doubly-linked chain by id. Must run after ids are set.
pub(crate) fn link_chunks(chunks: &mut [RagChunk]) {
    let ids: Vec<String> = chunks.iter().map(|c| c.chunk_id.clone()).collect();
    for (i, c) in chunks.iter_mut().enumerate() {
        c.prev_chunk_id = if i > 0 { Some(ids[i - 1].clone()) } else { None };
        c.next_chunk_id = ids.get(i + 1).cloned();
    }
}
```

- [ ] **Step 4: Add fields and call `link_chunks`**

In `rag.rs`:
- Add `use crate::pipeline::chunk_metadata::link_chunks;`.
- Add `pub prev_chunk_id: Option<String>,` and `pub next_chunk_id: Option<String>,` to `struct RagChunk`.
- Initialize both to `None` in `finalize` and `oversized_fragment`.
- In `DocxRagChunker::chunk`, after the id-stamping loop and before `chunks`:

```rust
        link_chunks(&mut chunks);
        chunks
```

- [ ] **Step 5: Run the new test and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: new test PASSES; all prior tests pass.

- [ ] **Step 6: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add oxidize-docx-core/src/pipeline/chunk_metadata.rs oxidize-docx-core/src/pipeline/rag.rs
git commit -m "feat(rag): link chunks with prev/next chunk ids"
```

---

## Task 11: Remap `ExtractionProfile` to pdf-mirrored names

Rename `Default`→`Standard`, `Minimal`→`Dense`, add `Rag`; keep `Academic`, `Technical`. Update the `apply_profile` matcher and migrate existing profile tests.

**Files:**
- Modify: `oxidize-docx-core/src/pipeline/profile.rs`
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Modify: `oxidize-docx-core/src/document.rs` (doc comment referencing `Minimal`)
- Test: `oxidize-docx-core/src/pipeline/rag.rs`, `oxidize-docx-core/tests/document_rag.rs` (if it names variants)

**Interfaces:**
- Produces: `pub enum ExtractionProfile { Standard, Rag, Academic, Dense, Technical }`, `#[default] = Standard`.

- [ ] **Step 1: Grep for existing variant references to migrate**

Run: `grep -rn "ExtractionProfile::\(Default\|Minimal\)" oxidize-docx-core`
Expected: a list of call sites (in `rag.rs` `apply_profile`, tests). Note them; every one must be updated in this task.

- [ ] **Step 2: Rewrite the enum**

Replace the whole `enum ExtractionProfile` in `profile.rs` with:

```rust
/// Selects how the chunker transforms the element stream before packing.
/// Names mirror oxidize-pdf's `ExtractionProfile` for ecosystem symmetry;
/// behavior is docx-specific (docx has no spatial partitioning to tune).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionProfile {
    /// Emit every element as-is; footnotes/endnotes/comments are their own
    /// blocks. General-purpose default.
    #[default]
    Standard,
    /// RAG-tuned: enables section-grouping (one chunk per heading section
    /// when it fits) and drops headers/footers. The recommended profile for
    /// building retrieval corpora.
    Rag,
    /// Inline footnote/endnote text into the referencing paragraph so each
    /// chunk carries its citations inline. For academic/scientific corpora.
    Academic,
    /// Drop footnotes, endnotes, and comments before chunking — only the main
    /// narrative survives (maximizes signal density).
    Dense,
    /// Keep tables intact regardless of token budget (never split a table).
    /// For technical/reference documents where table integrity matters.
    Technical,
}
```

- [ ] **Step 3: Update `apply_profile` in `rag.rs`**

In `rag.rs`, rewrite `apply_profile` so the match arms use the new names (behavior identical to before for the renamed variants; `Rag` and `Technical` pass elements through unchanged here — their behavior is wired in Task 12):

```rust
fn apply_profile(elements: &[DocxElement], profile: ExtractionProfile) -> Cow<'_, [DocxElement]> {
    match profile {
        ExtractionProfile::Standard | ExtractionProfile::Rag | ExtractionProfile::Technical => {
            Cow::Borrowed(elements)
        }
        ExtractionProfile::Dense => {
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
```

- [ ] **Step 4: Migrate existing profile tests**

In `rag.rs` tests, rename `ExtractionProfile::Default`→`Standard` and `ExtractionProfile::Minimal`→`Dense` in the existing tests `default_profile_produces_identical_chunks_to_no_profile_call` and `minimal_profile_drops_footnote_endnote_and_comment_elements`. Rename the second test to `dense_profile_drops_footnote_endnote_and_comment_elements`. Do the same in `oxidize-docx-core/tests/document_rag.rs` and any integration test flagged by Step 1's grep.

Also update the doc comment in `document.rs` that mentions `Minimal` to say `Dense`.

- [ ] **Step 5: Run the migrated tests and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: all tests pass with the new variant names.

- [ ] **Step 6: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add oxidize-docx-core/src/pipeline/profile.rs oxidize-docx-core/src/pipeline/rag.rs oxidize-docx-core/src/document.rs oxidize-docx-core/tests/document_rag.rs
git commit -m "feat(rag): remap ExtractionProfile to pdf-mirrored names (breaking)"
```

---

## Task 12: Wire `Rag` and `Technical` profile behavior

`Rag` enables section-grouping (headers/footers are already dropped by the packer). `Technical` keeps tables intact — a table whose estimate exceeds `max_tokens` is emitted atomically instead of… it already is, so `Technical` must *also* prevent the size cap from splitting nothing (tables are never sentence-split anyway). The observable `Technical` behavior: an oversized table is one chunk (already true) AND tables never merge (already true). To give `Technical` a *distinct, testable* behavior, it raises the effective table budget so an oversized table is not flagged `is_oversized`.

**Files:**
- Modify: `oxidize-docx-core/src/pipeline/rag.rs`
- Modify: `oxidize-docx-core/src/pipeline/hybrid.rs`
- Test: `oxidize-docx-core/src/pipeline/rag.rs`

**Interfaces:**
- Consumes: `DocxRagChunker.profile`, `group_sections`, `hybrid::pack*`.
- Produces (in `hybrid.rs`): `pack`/`pack_grouped` gain a `keep_tables_whole: bool` parameter controlling whether an over-budget table is flagged oversized.

- [ ] **Step 1: Write the failing tests**

Add to `rag.rs` tests:

```rust
#[test]
fn rag_profile_enables_section_grouping() {
    let elements = vec![
        DocxElement::Heading { level: 1, text: "Sec".into() },
        DocxElement::Paragraph { text: "a".into(), parent_heading: None, links: vec![] },
        DocxElement::Paragraph { text: "b".into(), parent_heading: None, links: vec![] },
    ];
    // Rag profile groups the whole fitting section into one chunk without
    // the caller having to opt in via with_section_grouping.
    let chunks = DocxRagChunker::new()
        .with_profile(ExtractionProfile::Rag)
        .chunk(&elements);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].text, "Sec\n\na\n\nb");
}

#[test]
fn technical_profile_keeps_oversized_table_unflagged() {
    use crate::pipeline::element::{TableCell, TableRow};
    // A table whose text estimate exceeds max_tokens=2.
    let table = DocxElement::Table {
        rows: vec![TableRow {
            cells: vec![
                TableCell { text: "one two three".into(), col_span: 1, row_span: 1 },
                TableCell { text: "four five six".into(), col_span: 1, row_span: 1 },
            ],
        }],
    };
    let standard = DocxRagChunker::new().with_max_tokens(2).chunk(&[table.clone()]);
    assert!(standard[0].is_oversized, "Standard flags the over-budget table");

    let technical = DocxRagChunker::new()
        .with_max_tokens(2)
        .with_profile(ExtractionProfile::Technical)
        .chunk(&[table]);
    assert_eq!(technical.len(), 1);
    assert!(!technical[0].is_oversized, "Technical keeps the table whole, unflagged");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p oxidize-docx-core -- rag_profile_enables technical_profile_keeps --nocapture`
Expected: FAIL — Rag doesn't group; Technical flags the table oversized.

- [ ] **Step 3: Thread `keep_tables_whole` through the packers**

In `hybrid.rs`, change signatures to
`pub(crate) fn pack(elements: &[DocxElement], max_tokens: usize, policy: MergePolicy, keep_tables_whole: bool)`
and
`pub(crate) fn pack_grouped(elements: &[DocxElement], max_tokens: usize, policy: MergePolicy, keep_tables_whole: bool)`.
In `pack`'s table branch, change the oversized decision:

```rust
            if elem_tokens > max_tokens && !keep_tables_whole {
                out.push(RagChunk::oversized_fragment(
                    text, i, "table", heading_stack.clone(), elem_tokens,
                ));
            } else {
                let mut acc = ChunkAccumulator::default();
                acc.push(i, text, "table");
                out.push(acc.finalize(heading_stack.clone()));
            }
```

Propagate `keep_tables_whole` from `pack_grouped` into its internal `pack` calls, and pass `false` from `single_chunk`'s section-fits path (not applicable there — sections that fit are never oversized).

- [ ] **Step 4: Derive the flags from the profile in `chunk_view`**

In `rag.rs`, update `chunk_view` to compute effective flags:

```rust
    fn chunk_view(&self, elements: &[DocxElement]) -> Vec<RagChunk> {
        let group = self.group_sections || self.profile == ExtractionProfile::Rag;
        let keep_tables_whole = self.profile == ExtractionProfile::Technical;
        if group {
            crate::pipeline::hybrid::pack_grouped(
                elements, self.max_tokens, self.merge_policy, keep_tables_whole,
            )
        } else {
            crate::pipeline::hybrid::pack(
                elements, self.max_tokens, self.merge_policy, keep_tables_whole,
            )
        }
    }
```

- [ ] **Step 5: Fix earlier call sites**

Update the Task-6 direct-call tests and any other `hybrid::pack`/`pack_grouped` callers to pass the new `keep_tables_whole` argument (`false`). Run the grep: `grep -rn "hybrid::pack" oxidize-docx-core/src` and fix each.

- [ ] **Step 6: Run the new tests and the full suite**

Run: `cargo test -p oxidize-docx-core`
Expected: both new tests PASS; all prior tests pass.

- [ ] **Step 7: Clippy + fmt**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add oxidize-docx-core/src/pipeline/hybrid.rs oxidize-docx-core/src/pipeline/rag.rs
git commit -m "feat(rag): wire Rag (section-grouping) and Technical (tables intact) profiles"
```

---

## Task 13: Integration test over real fixtures + `empty_chunks` check

Validate cap + unique ids against real Word documents, and assert no empty-text chunks (the known `empty_chunks` noise the roadmap flags).

**Files:**
- Create/Modify: `oxidize-docx-core/tests/document_rag.rs`

**Interfaces:**
- Consumes: `DocxDocument::open`, `rag_chunks`.

- [ ] **Step 1: Confirm a usable fixture path**

Run: `ls oxidize-docx-core/tests/fixtures/*.docx`
Expected: at least `quarterly_report.docx`. Use it (the `.private/fixtures/` corpus is gitignored and not committed; the committed fixture is the reproducible one for CI).

- [ ] **Step 2: Write the failing test**

Add to `oxidize-docx-core/tests/document_rag.rs`:

```rust
#[test]
fn rag_chunks_respect_cap_and_have_unique_nonempty_ids() {
    let doc = oxidize_docx_core::DocxDocument::open(
        "tests/fixtures/quarterly_report.docx",
    )
    .expect("fixture opens");
    let chunks = doc.rag_chunks().expect("chunks");
    assert!(!chunks.is_empty(), "fixture yields chunks");

    // No empty-text chunks (roadmap empty_chunks noise must not appear).
    for c in &chunks {
        assert!(
            !c.text.trim().is_empty(),
            "chunk {} has empty text",
            c.chunk_index
        );
    }

    // Every non-oversized chunk respects the 800-token default cap.
    for c in &chunks {
        if !c.is_oversized {
            assert!(
                c.token_estimate <= 800,
                "chunk {} exceeds cap: {}",
                c.chunk_index,
                c.token_estimate
            );
        }
    }

    // chunk_ids are unique.
    let mut ids: Vec<&str> = chunks.iter().map(|c| c.chunk_id.as_str()).collect();
    ids.sort_unstable();
    let unique = {
        let mut d = ids.clone();
        d.dedup();
        d.len()
    };
    assert_eq!(unique, ids.len(), "chunk_ids must be unique");
}
```

- [ ] **Step 3: Run test**

Run: `cargo test -p oxidize-docx-core --test document_rag rag_chunks_respect_cap -- --nocapture`
Expected: PASS. If the empty-text assert fails, that reproduces the `empty_chunks` bug — fix by filtering elements whose extracted text is empty in `hybrid::text_and_type` (return `None` when `text.trim().is_empty()` for paragraph/list_item). Re-run until green, then include that fix in this commit.

- [ ] **Step 4: If the empty-text fix was needed, apply it**

In `hybrid.rs` `text_and_type`, guard the text-bearing arms:

```rust
        DocxElement::Paragraph { text, .. } if text.trim().is_empty() => None,
        DocxElement::ListItem { text, .. } if text.trim().is_empty() => None,
```

placed before the corresponding non-guarded arms. Re-run Step 3.

- [ ] **Step 5: Full suite + clippy + fmt**

Run: `cargo test -p oxidize-docx-core && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean, all green.

- [ ] **Step 6: Commit**

```bash
git add oxidize-docx-core/tests/document_rag.rs oxidize-docx-core/src/pipeline/hybrid.rs
git commit -m "test(rag): integration cap + unique-id + no-empty-chunk over real fixture"
```

---

## Task 14: Documentation — CLAUDE.md + ROADMAP.md

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/ROADMAP.md`

**Interfaces:** none (docs).

- [ ] **Step 1: Correct the CLAUDE.md claim**

In `CLAUDE.md`, in the pipeline description, change the Stage 4 line that reads
`Stage 4: RAG Chunking → Vec<RagChunk> (hybrid chunker, misma implementación que oxidize-pdf)`
to:

```
Stage 4: RAG Chunking         → Vec<RagChunk> (hybrid chunker con paridad de algoritmo/metadata con oxidize-pdf; token model propio word*1.5)
```

- [ ] **Step 2: Update the roadmap**

In `docs/ROADMAP.md`, in the Fase 4 `DocxRagChunker` line, remove the "Pendiente: agresividad de chunking inter-elemento…" note and append:

```
Cap inter-elemento implementado (2026-07-02, feature/rag-align-oxidize-pdf): buffer flushea cuando el siguiente elemento excedería max_tokens. Añadidos MergePolicy, section-grouping, metadata chunk_id/prev-next/heading_path/content-flags, profiles espejo de pdf (Standard/Rag/Academic/Dense/Technical). Token model x1.5 conservado.
```

Update the "Última revisión" date at the top to `2026-07-02`.

- [ ] **Step 3: Verify docs build / no broken references**

Run: `cargo test -p oxidize-docx-core --doc`
Expected: doctests (if any) pass; docs unaffected.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md docs/ROADMAP.md
git commit -m "docs(rag): correct oxidize-pdf parity claim and record alignment"
```

---

## Final verification (before PR)

- [ ] Run the full gate:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: fmt clean, clippy clean, all tests green (242 prior + the new ones).

- [ ] Confirm public API exports resolve:

Run: `grep -n "MergePolicy\|ContentTypeFlags\|ExtractionProfile" oxidize-docx-core/src/pipeline/mod.rs`
Expected: `MergePolicy`, `ContentTypeFlags`, `ExtractionProfile` all re-exported.

- [ ] Open PR `feature/rag-align-oxidize-pdf` → `develop` (Gitflow). Do NOT merge to `main` or tag without explicit authorization.

---

## Self-Review (author checklist — completed)

**Spec coverage:** cap (T3), structural break (T4), MergePolicy (T5), section-grouping (T6), ContentTypeFlags (T7), heading_path+full_text (T8), chunk_id+index (T9), prev/next (T10), profile remap (T11), Rag/Technical behavior (T12), integration+empty_chunks (T13), docs+version (T1/T14), sha2 dep (T2). All spec sections mapped.

**Placeholder scan:** no TBD/TODO in steps; every code step shows full code.

**Type consistency:** `pack(elements, max_tokens, policy, keep_tables_whole)` and `pack_grouped(...)` signatures are introduced incrementally (T3 → T5 adds `policy` → T12 adds `keep_tables_whole`); each task that changes the signature updates all call sites in the same task (T5 Step 5-equivalent via chunk_view; T12 Step 5 greps and fixes). `RagChunk` fields accrete across T7–T10, each initialized in both constructors (`finalize`, `oversized_fragment`). `ContentTypeFlags`, `MergePolicy`, `ExtractionProfile` names consistent across tasks and re-exports.
