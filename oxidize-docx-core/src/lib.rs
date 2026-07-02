pub mod document;
pub mod error;
pub mod images;
pub mod numbering;
pub(crate) mod ooxml;
pub mod pipeline;
pub(crate) mod raw;
pub(crate) mod styles;
pub(crate) mod word;
pub(crate) mod xml;
pub(crate) mod zip;

pub use document::DocxDocument;
pub use error::{DocxError, Result};
pub use images::ImageMetadata;
pub use pipeline::DocxElement;
