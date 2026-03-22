pub(crate) mod security;

use std::io::{BufReader, Read};
use std::path::Path;

use crate::error::{DocxError, Result};
use security::{
    validate_entry_count, validate_entry_size, validate_zip_path, MAX_TOTAL_UNCOMPRESSED_SIZE,
};

pub(crate) struct SecureZipArchive {
    inner: zip::ZipArchive<BufReader<std::fs::File>>,
}

impl SecureZipArchive {
    /// Opens a ZIP file with security validation (entry count limit).
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = BufReader::new(file);
        let archive = zip::ZipArchive::new(reader)?;

        validate_entry_count(archive.len())?;

        Ok(Self { inner: archive })
    }

    /// Reads a ZIP entry by name with security validation (path traversal, size, ratio).
    pub(crate) fn read_entry(&mut self, name: &str) -> Result<Vec<u8>> {
        validate_zip_path(name)?;

        let mut entry = self.inner.by_name(name).map_err(|e| match e {
            zip::result::ZipError::FileNotFound => DocxError::MissingPart(name.to_string()),
            other => DocxError::Zip(other),
        })?;

        validate_entry_size(name, entry.size(), entry.compressed_size())?;

        // Guard total uncompressed size
        if entry.size() > MAX_TOTAL_UNCOMPRESSED_SIZE {
            return Err(DocxError::ZipBomb {
                entry: name.to_string(),
                claimed_size: entry.size(),
                limit: MAX_TOTAL_UNCOMPRESSED_SIZE,
            });
        }

        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// Returns all entry names in the archive.
    #[allow(dead_code)]
    pub(crate) fn entry_names(&self) -> Vec<String> {
        (0..self.inner.len())
            .filter_map(|i| self.inner.name_for_index(i).map(|name| name.to_string()))
            .collect()
    }

    /// Returns true if the archive contains an entry with the given name.
    #[allow(dead_code)]
    pub(crate) fn has_entry(&self, name: &str) -> bool {
        self.entry_names().iter().any(|n| n == name)
    }
}
