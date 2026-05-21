pub(crate) mod formatting;
pub(crate) mod resolver;
pub(crate) mod table;

#[allow(unused_imports)]
pub(crate) use formatting::ResolvedFormatting;
#[allow(unused_imports)]
pub(crate) use resolver::StyleResolver;
#[allow(unused_imports)]
pub(crate) use table::{StyleEntry, StyleTable, StyleType};
