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
}
