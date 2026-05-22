pub(crate) mod classifier;
pub mod element;

pub use crate::numbering::ListType;
#[allow(unused_imports)]
pub(crate) use classifier::ClassifierPipeline;
pub use element::{DocxElement, HeadingContext};
