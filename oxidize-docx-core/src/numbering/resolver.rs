use std::collections::HashMap;

use crate::error::Result;
use crate::numbering::defs::NumberingDefs;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ListType {
    Bullet,
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerLetter,
    UpperLetter,
    None,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct ListItemInfo {
    pub(crate) num_id: u32,
    pub(crate) ilvl: u8,
    pub(crate) list_type: ListType,
    pub(crate) display_index: Option<u32>,
    pub(crate) level_text: String,
}

#[allow(dead_code)]
pub(crate) struct NumberingResolver<'a> {
    defs: &'a NumberingDefs,
    counters: HashMap<(u32, u8), u32>,
}

#[allow(dead_code)]
impl<'a> NumberingResolver<'a> {
    pub(crate) fn new(defs: &'a NumberingDefs) -> Self {
        Self {
            defs,
            counters: HashMap::new(),
        }
    }

    pub(crate) fn advance(&mut self, num_id: u32, ilvl: u8) -> Result<ListItemInfo> {
        let level = self.defs.resolve(num_id, ilvl)?;
        let list_type = ListType::from_num_fmt(&level.num_fmt);
        let level_text = level.level_text.clone();
        let start = level.start;

        self.counters
            .retain(|(nid, lvl), _| !(*nid == num_id && *lvl > ilvl));

        let next = match self.counters.get(&(num_id, ilvl)) {
            Some(curr) => curr + 1,
            None => start,
        };
        self.counters.insert((num_id, ilvl), next);

        let display_index = match list_type {
            ListType::Bullet | ListType::None => None,
            _ => Some(next),
        };

        Ok(ListItemInfo {
            num_id,
            ilvl,
            list_type,
            display_index,
            level_text,
        })
    }
}

impl ListType {
    fn from_num_fmt(num_fmt: &str) -> ListType {
        match num_fmt {
            "decimal" => ListType::Decimal,
            "bullet" => ListType::Bullet,
            "lowerRoman" => ListType::LowerRoman,
            "none" => ListType::None,
            other => ListType::Other(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DocxError;
    use crate::numbering::defs::{AbstractNum, ConcreteNum, NumberingLevel};

    fn level(ilvl: u8, num_fmt: &str, start: u32) -> NumberingLevel {
        NumberingLevel {
            ilvl,
            start,
            num_fmt: num_fmt.into(),
            level_text: format!("%{}.", ilvl + 1),
            indent_left: None,
            indent_hanging: None,
        }
    }

    /// Build a NumberingDefs with a single concrete num bound to an abstract
    /// num that has the given levels.
    fn defs_with(num_id: u32, levels: Vec<NumberingLevel>) -> NumberingDefs {
        let mut defs = NumberingDefs::new();
        defs.insert_abstract(AbstractNum {
            abstract_num_id: 0,
            levels,
        });
        defs.insert_concrete(ConcreteNum {
            num_id,
            abstract_num_id: 0,
            level_overrides: vec![],
        });
        defs
    }

    fn defs_two_nums() -> NumberingDefs {
        let mut defs = NumberingDefs::new();
        defs.insert_abstract(AbstractNum {
            abstract_num_id: 0,
            levels: vec![level(0, "decimal", 1), level(1, "decimal", 1)],
        });
        defs.insert_concrete(ConcreteNum {
            num_id: 1,
            abstract_num_id: 0,
            level_overrides: vec![],
        });
        defs.insert_concrete(ConcreteNum {
            num_id: 2,
            abstract_num_id: 0,
            level_overrides: vec![],
        });
        defs
    }

    #[test]
    fn advance_with_unknown_num_id_returns_numbering_def_not_found() {
        let defs = defs_with(1, vec![level(0, "decimal", 1)]);
        let mut r = NumberingResolver::new(&defs);

        let result = r.advance(99, 0);
        match result {
            Err(DocxError::NumberingDefNotFound { num_id }) => {
                assert_eq!(num_id, 99);
            }
            Ok(v) => panic!("expected NumberingDefNotFound, got Ok({v:?})"),
            Err(e) => panic!("expected NumberingDefNotFound, got {e:?}"),
        }
    }

    #[test]
    fn deeper_level_reset_does_not_wipe_other_num_ids() {
        // Advancing a different num_id at a shallower ilvl must NOT reset the
        // deeper-level counters of an unrelated num_id. Two lists are
        // independent state machines.
        let defs = defs_two_nums();
        let mut r = NumberingResolver::new(&defs);

        let one_zero = r.advance(1, 0).unwrap();
        let one_one_a = r.advance(1, 1).unwrap();
        let two_zero = r.advance(2, 0).unwrap();
        let one_one_b = r.advance(1, 1).unwrap();

        assert_eq!(one_zero.display_index, Some(1));
        assert_eq!(one_one_a.display_index, Some(1));
        assert_eq!(two_zero.display_index, Some(1));
        assert_eq!(
            one_one_b.display_index,
            Some(2),
            "advancing num_id=2 must not reset num_id=1 ilvl=1 counter"
        );
    }

    #[test]
    fn level_start_above_one_seeds_first_index() {
        let defs = defs_with(1, vec![level(0, "decimal", 5)]);
        let mut r = NumberingResolver::new(&defs);

        let a = r.advance(1, 0).unwrap();
        let b = r.advance(1, 0).unwrap();

        assert_eq!(
            a.display_index,
            Some(5),
            "first index must equal level.start"
        );
        assert_eq!(b.display_index, Some(6));
    }

    #[test]
    fn list_type_maps_bullet_decimal_lower_roman_and_bullet_has_no_index() {
        let bullet_defs = defs_with(1, vec![level(0, "bullet", 1)]);
        let mut br = NumberingResolver::new(&bullet_defs);
        let b = br.advance(1, 0).unwrap();
        assert_eq!(b.list_type, ListType::Bullet);
        assert_eq!(
            b.display_index, None,
            "bullets must not carry a numeric index"
        );

        let decimal_defs = defs_with(2, vec![level(0, "decimal", 1)]);
        let mut dr = NumberingResolver::new(&decimal_defs);
        let d = dr.advance(2, 0).unwrap();
        assert_eq!(d.list_type, ListType::Decimal);
        assert_eq!(d.display_index, Some(1));

        let roman_defs = defs_with(3, vec![level(0, "lowerRoman", 1)]);
        let mut rr = NumberingResolver::new(&roman_defs);
        let r = rr.advance(3, 0).unwrap();
        assert_eq!(r.list_type, ListType::LowerRoman);
        assert_eq!(r.display_index, Some(1));
    }

    #[test]
    fn shallower_advance_resets_deeper_levels() {
        // Word semantics: when a list re-advances at a shallower ilvl, the
        // counters for deeper ilvls of the SAME num_id are reset, so the
        // next deeper item starts at `start` again.
        let defs = defs_with(1, vec![level(0, "decimal", 1), level(1, "decimal", 1)]);
        let mut r = NumberingResolver::new(&defs);

        let seq = [(1, 0), (1, 1), (1, 1), (1, 0), (1, 1)];
        let got: Vec<Option<u32>> = seq
            .iter()
            .map(|(n, l)| r.advance(*n, *l).unwrap().display_index)
            .collect();

        assert_eq!(
            got,
            vec![Some(1), Some(1), Some(2), Some(2), Some(1)],
            "ilvl=1 counter must reset after the second ilvl=0 advance"
        );
    }

    #[test]
    fn three_advances_at_same_level_yield_1_2_3() {
        let defs = defs_with(1, vec![level(0, "decimal", 1)]);
        let mut resolver = NumberingResolver::new(&defs);

        let a = resolver.advance(1, 0).expect("first advance");
        let b = resolver.advance(1, 0).expect("second advance");
        let c = resolver.advance(1, 0).expect("third advance");

        assert_eq!(a.display_index, Some(1), "first item must be 1");
        assert_eq!(b.display_index, Some(2), "second item must be 2");
        assert_eq!(c.display_index, Some(3), "third item must be 3");
        assert_eq!(a.list_type, ListType::Decimal);
        assert_eq!(a.num_id, 1);
        assert_eq!(a.ilvl, 0);
        assert_eq!(a.level_text, "%1.");
    }
}
