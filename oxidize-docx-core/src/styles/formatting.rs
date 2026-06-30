#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct ResolvedFormatting {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
    pub(crate) font_size_half_points: Option<u32>,
    pub(crate) color: Option<String>,
    pub(crate) outline_level: Option<u8>,
    pub(crate) heading_level: Option<u8>,
    /// Numbering reference resolved through the style chain (docDefaults →
    /// basedOn chain → direct pPr). Word's built-in list styles carry the
    /// numPr on the style, not the paragraph, so this is how a paragraph
    /// styled "List Bullet"/"List Number" surfaces as a list item.
    pub(crate) num_pr: Option<crate::raw::paragraphs::RawNumPr>,
}
