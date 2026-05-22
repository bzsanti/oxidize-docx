use super::drawing::RawDrawing;
use super::fields::RawFieldInst;
use super::runs::RawRun;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawNumPr {
    pub(crate) num_id: u32,
    pub(crate) ilvl: u8,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct RawParagraphProperties {
    pub(crate) style_id: Option<String>,
    pub(crate) num_pr: Option<RawNumPr>,
    pub(crate) alignment: Option<String>,
    pub(crate) outline_level: Option<u8>,
    pub(crate) keep_next: bool,
    pub(crate) page_break_before: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawHyperlink {
    pub(crate) rel_id: Option<String>,
    pub(crate) anchor: Option<String>,
    pub(crate) runs: Vec<RawRun>,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct RawParagraph {
    pub(crate) properties: RawParagraphProperties,
    pub(crate) runs: Vec<RawRun>,
    pub(crate) hyperlinks: Vec<RawHyperlink>,
    pub(crate) drawings: Vec<RawDrawing>,
    pub(crate) fields: Vec<RawFieldInst>,
    /// IDs of every `<w:footnoteReference w:id="N"/>` encountered inside
    /// the paragraph's runs, in document order. Used by the classifier
    /// to look up footnote text in `FootnoteMap`.
    pub(crate) footnote_ref_ids: Vec<u32>,
    /// IDs of every `<w:endnoteReference w:id="N"/>` encountered inside
    /// the paragraph's runs, in document order. Used by the classifier
    /// to look up endnote text in `EndnoteMap`.
    pub(crate) endnote_ref_ids: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::runs::RawRunProperties;

    #[test]
    fn paragraph_props_default() {
        let ppr = RawParagraphProperties::default();
        assert!(ppr.style_id.is_none());
        assert!(ppr.num_pr.is_none());
        assert!(ppr.alignment.is_none());
        assert!(ppr.outline_level.is_none());
        assert!(!ppr.keep_next);
        assert!(!ppr.page_break_before);
    }

    #[test]
    fn num_pr_values() {
        let np = RawNumPr { num_id: 1, ilvl: 0 };
        assert_eq!(np.num_id, 1);
        assert_eq!(np.ilvl, 0);
    }

    #[test]
    fn paragraph_has_runs() {
        let p = RawParagraph {
            runs: vec![
                RawRun {
                    text: Some("Hello ".into()),
                    properties: RawRunProperties::default(),
                },
                RawRun {
                    text: Some("World".into()),
                    properties: RawRunProperties::default(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(p.runs.len(), 2);
    }

    #[test]
    fn paragraph_with_hyperlink() {
        let p = RawParagraph {
            hyperlinks: vec![RawHyperlink {
                rel_id: Some("rId5".into()),
                anchor: None,
                runs: vec![RawRun {
                    text: Some("Click here".into()),
                    properties: RawRunProperties::default(),
                }],
            }],
            ..Default::default()
        };
        assert_eq!(p.hyperlinks.len(), 1);
        assert_eq!(p.hyperlinks[0].rel_id.as_deref(), Some("rId5"));
        assert_eq!(p.hyperlinks[0].runs[0].text.as_deref(), Some("Click here"));
    }

    #[test]
    fn paragraph_with_style_and_numbering() {
        let ppr = RawParagraphProperties {
            style_id: Some("ListParagraph".into()),
            num_pr: Some(RawNumPr { num_id: 2, ilvl: 1 }),
            alignment: Some("center".into()),
            ..Default::default()
        };
        assert_eq!(ppr.style_id.as_deref(), Some("ListParagraph"));
        assert_eq!(ppr.num_pr.as_ref().unwrap().num_id, 2);
        assert_eq!(ppr.num_pr.as_ref().unwrap().ilvl, 1);
        assert_eq!(ppr.alignment.as_deref(), Some("center"));
    }
}
