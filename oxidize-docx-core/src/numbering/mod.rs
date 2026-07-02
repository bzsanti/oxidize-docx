pub(crate) mod defs;
pub(crate) mod resolver;

#[allow(unused_imports)]
pub(crate) use defs::{AbstractNum, ConcreteNum, NumberingDefs, NumberingLevel};
pub use resolver::ListType;
#[allow(unused_imports)]
pub(crate) use resolver::{ListItemInfo, NumberingResolver};
