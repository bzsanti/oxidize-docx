#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawDrawing {
    pub(crate) rel_id: String,
    pub(crate) width_emu: Option<u64>,
    pub(crate) height_emu: Option<u64>,
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) is_inline: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_drawing_has_rel_id() {
        let d = RawDrawing {
            rel_id: "rId3".into(),
            width_emu: Some(1234),
            height_emu: Some(5678),
            title: None,
            description: None,
            is_inline: true,
        };
        assert_eq!(d.rel_id, "rId3");
        assert_eq!(d.width_emu, Some(1234));
        assert_eq!(d.height_emu, Some(5678));
        assert!(d.is_inline);
    }

    #[test]
    fn raw_drawing_anchor_not_inline() {
        let d = RawDrawing {
            rel_id: "rId5".into(),
            width_emu: None,
            height_emu: None,
            title: Some("Logo".into()),
            description: Some("Company logo".into()),
            is_inline: false,
        };
        assert!(!d.is_inline);
        assert_eq!(d.title.as_deref(), Some("Logo"));
        assert_eq!(d.description.as_deref(), Some("Company logo"));
    }
}
