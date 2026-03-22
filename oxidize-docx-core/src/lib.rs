pub mod document;
pub mod error;
pub(crate) mod ooxml;
pub(crate) mod xml;
pub(crate) mod zip;

pub use document::DocxDocument;
pub use error::{DocxError, Result};
