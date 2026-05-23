use super::runs::RawRun;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RawFieldInst {
    pub(crate) instruction: String,
    pub(crate) runs: Vec<RawRun>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::runs::RawRunProperties;

    #[test]
    fn raw_field_inst_hyperlink() {
        let f = RawFieldInst {
            instruction: "HYPERLINK \"https://example.com\"".into(),
            runs: vec![],
        };
        assert!(f.instruction.contains("HYPERLINK"));
        assert!(f.runs.is_empty());
    }

    #[test]
    fn raw_field_inst_with_runs() {
        let f = RawFieldInst {
            instruction: "TOC \\o \"1-3\"".into(),
            runs: vec![RawRun {
                text: Some("Table of Contents".into()),
                properties: RawRunProperties::default(),
            }],
        };
        assert!(f.instruction.contains("TOC"));
        assert_eq!(f.runs.len(), 1);
        assert_eq!(f.runs[0].text.as_deref(), Some("Table of Contents"));
    }
}
