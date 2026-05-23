/// Knob that tells the chunker how aggressively to fold annotations and
/// auxiliary content into the main text stream when emitting RAG chunks.
/// The profile only affects chunking — `DocxDocument::elements()` always
/// returns the full semantic stream regardless of profile selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionProfile {
    /// Emit every element as-is. The chunker treats footnotes, endnotes,
    /// and comments as their own blocks tagged with their `element_type`.
    /// Right default for general-purpose RAG over arbitrary documents.
    #[default]
    Default,
    /// Inline footnote and endnote text into the paragraph that
    /// referenced them so each chunk carries its citations as part of
    /// the prose. Useful for academic / scientific corpora where the
    /// surrounding sentence loses meaning without the footnote.
    Academic,
    /// Same as `Default` today. Reserved for future technical-document
    /// tuning (e.g., promoting code blocks or keeping tables intact
    /// regardless of token budget). Selecting it now is forward-compatible.
    Technical,
    /// Drop footnotes, endnotes, and comments before chunking so the
    /// resulting chunks contain only the main narrative — useful for
    /// search / index pipelines that should not surface marginalia.
    Minimal,
}
