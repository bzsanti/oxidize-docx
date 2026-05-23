use std::collections::HashMap;

use crate::error::{DocxError, Result};
use crate::raw::paragraphs::RawParagraphProperties;
use crate::raw::runs::RawRunProperties;

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct NumberingLevel {
    pub(crate) ilvl: u8,
    pub(crate) start: u32,
    pub(crate) num_fmt: String,
    pub(crate) level_text: String,
    pub(crate) indent_left: Option<u32>,
    pub(crate) indent_hanging: Option<u32>,
    /// Run properties declared inside `<w:lvl>/<w:rPr>`. These form the
    /// list-level layer of the 4-layer style chain
    /// (docDefaults → basedOn chain → list-level → direct).
    pub(crate) run_properties: Option<RawRunProperties>,
    /// Paragraph properties declared inside `<w:lvl>/<w:pPr>`.
    pub(crate) paragraph_properties: Option<RawParagraphProperties>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct AbstractNum {
    pub(crate) abstract_num_id: u32,
    pub(crate) levels: Vec<NumberingLevel>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct NumberingLevelOverride {
    pub(crate) ilvl: u8,
    pub(crate) start_override: Option<u32>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ConcreteNum {
    pub(crate) num_id: u32,
    pub(crate) abstract_num_id: u32,
    pub(crate) level_overrides: Vec<NumberingLevelOverride>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct NumberingDefs {
    abstract_nums: HashMap<u32, AbstractNum>,
    concrete_nums: HashMap<u32, ConcreteNum>,
}

#[allow(dead_code)]
impl NumberingDefs {
    pub(crate) fn new() -> Self {
        Self {
            abstract_nums: HashMap::new(),
            concrete_nums: HashMap::new(),
        }
    }

    pub(crate) fn insert_abstract(&mut self, an: AbstractNum) {
        self.abstract_nums.insert(an.abstract_num_id, an);
    }

    pub(crate) fn insert_concrete(&mut self, cn: ConcreteNum) {
        self.concrete_nums.insert(cn.num_id, cn);
    }

    /// Resolves a numbering reference (numId + ilvl) to the corresponding level definition.
    pub(crate) fn resolve(&self, num_id: u32, ilvl: u8) -> Result<&NumberingLevel> {
        let concrete = self
            .concrete_nums
            .get(&num_id)
            .ok_or(DocxError::NumberingDefNotFound { num_id })?;

        let abstract_num = self.abstract_nums.get(&concrete.abstract_num_id).ok_or(
            DocxError::AbstractNumNotFound {
                abstract_num_id: concrete.abstract_num_id,
            },
        )?;

        abstract_num
            .levels
            .iter()
            .find(|l| l.ilvl == ilvl)
            .ok_or(DocxError::NumberingDefNotFound { num_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_level(ilvl: u8) -> NumberingLevel {
        NumberingLevel {
            ilvl,
            start: 1,
            num_fmt: "decimal".into(),
            level_text: format!("%{}.", ilvl),
            ..Default::default()
        }
    }

    fn sample_defs() -> NumberingDefs {
        let mut defs = NumberingDefs::new();
        defs.insert_abstract(AbstractNum {
            abstract_num_id: 0,
            levels: vec![sample_level(0), sample_level(1), sample_level(2)],
        });
        defs.insert_concrete(ConcreteNum {
            num_id: 1,
            abstract_num_id: 0,
            level_overrides: vec![],
        });
        defs
    }

    #[test]
    fn numbering_level_defaults() {
        let level = sample_level(0);
        assert_eq!(level.start, 1);
        assert_eq!(level.num_fmt, "decimal");
        assert_eq!(level.ilvl, 0);
    }

    #[test]
    fn abstract_num_has_levels() {
        let an = AbstractNum {
            abstract_num_id: 0,
            levels: vec![sample_level(0)],
        };
        assert_eq!(an.levels.len(), 1);
    }

    #[test]
    fn concrete_num_links_abstract() {
        let cn = ConcreteNum {
            num_id: 1,
            abstract_num_id: 0,
            level_overrides: vec![],
        };
        assert_eq!(cn.num_id, 1);
        assert_eq!(cn.abstract_num_id, 0);
    }

    #[test]
    fn resolve_num_id() {
        let defs = sample_defs();
        let level = defs.resolve(1, 0).unwrap();
        assert_eq!(level.num_fmt, "decimal");
        assert_eq!(level.start, 1);
        assert_eq!(level.ilvl, 0);
    }

    #[test]
    fn resolve_deeper_level() {
        let defs = sample_defs();
        let level = defs.resolve(1, 2).unwrap();
        assert_eq!(level.ilvl, 2);
    }

    #[test]
    fn resolve_missing_num_id_returns_error() {
        let defs = sample_defs();
        let result = defs.resolve(99, 0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DocxError::NumberingDefNotFound { num_id: 99 }
        ));
    }

    #[test]
    fn resolve_missing_abstract_returns_error() {
        let mut defs = NumberingDefs::new();
        defs.insert_concrete(ConcreteNum {
            num_id: 1,
            abstract_num_id: 999,
            level_overrides: vec![],
        });
        let result = defs.resolve(1, 0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DocxError::AbstractNumNotFound {
                abstract_num_id: 999
            }
        ));
    }

    #[test]
    fn resolve_missing_ilvl_returns_error() {
        let defs = sample_defs();
        let result = defs.resolve(1, 9); // only levels 0-2 exist
        assert!(result.is_err());
    }
}
