pub(crate) mod classifier;
pub mod element;
pub mod list_builder;
pub(crate) mod table_builder;

pub use crate::numbering::ListType;
#[allow(unused_imports)]
pub(crate) use classifier::ClassifierPipeline;
pub use element::{DocxElement, HeadingContext, TableCell, TableRow};
pub use list_builder::{nest_list_items, NestedList, NestedListItem};
