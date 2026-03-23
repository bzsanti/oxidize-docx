pub mod document;
pub mod error;
pub(crate) mod numbering;
pub(crate) mod ooxml;
pub(crate) mod raw;
pub(crate) mod styles;
pub(crate) mod word;
pub(crate) mod xml;
pub(crate) mod zip;

pub use document::DocxDocument;
pub use error::{DocxError, Result};
