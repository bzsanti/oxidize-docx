pub(crate) mod classifier;
pub mod element;
pub mod export;
pub(crate) mod hybrid;
pub mod list_builder;
pub mod profile;
pub mod rag;
pub(crate) mod table_builder;

pub use crate::numbering::ListType;
#[allow(unused_imports)]
pub(crate) use classifier::ClassifierPipeline;
pub use element::{DocxElement, HeaderKind, HeadingContext, LinkSpan, TableCell, TableRow};
pub use export::{to_markdown, to_plain_text};
pub use hybrid::MergePolicy;
pub use list_builder::{nest_list_items, NestedList, NestedListItem};
pub use profile::ExtractionProfile;
pub use rag::{DocxRagChunker, RagChunk};
