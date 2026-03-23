#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct RawRunProperties {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
    pub(crate) font_size_half_points: Option<u32>,
    pub(crate) color: Option<String>,
    pub(crate) highlight: Option<String>,
    pub(crate) vertical_align: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawRun {
    pub(crate) text: Option<String>,
    pub(crate) properties: RawRunProperties,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_run_properties_default() {
        let rpr = RawRunProperties::default();
        assert!(!rpr.bold);
        assert!(!rpr.italic);
        assert!(!rpr.underline);
        assert!(!rpr.strikethrough);
        assert!(rpr.font_size_half_points.is_none());
        assert!(rpr.color.is_none());
        assert!(rpr.highlight.is_none());
        assert!(rpr.vertical_align.is_none());
    }

    #[test]
    fn raw_run_with_text() {
        let run = RawRun {
            text: Some("Hello".into()),
            properties: RawRunProperties::default(),
        };
        assert_eq!(run.text.as_deref(), Some("Hello"));
    }

    #[test]
    fn raw_run_break_has_no_text() {
        let run = RawRun {
            text: None,
            properties: RawRunProperties::default(),
        };
        assert!(run.text.is_none());
    }

    #[test]
    fn raw_run_properties_with_formatting() {
        let rpr = RawRunProperties {
            bold: true,
            italic: true,
            font_size_half_points: Some(24),
            color: Some("FF0000".into()),
            ..Default::default()
        };
        assert!(rpr.bold);
        assert!(rpr.italic);
        assert_eq!(rpr.font_size_half_points, Some(24));
        assert_eq!(rpr.color.as_deref(), Some("FF0000"));
    }
}
