use std::collections::HashMap;

use crate::raw::paragraphs::RawParagraphProperties;
use crate::raw::runs::RawRunProperties;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum StyleType {
    Paragraph,
    Character,
    Table,
    Numbering,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct StyleEntry {
    pub(crate) style_id: String,
    pub(crate) name: String,
    pub(crate) style_type: StyleType,
    pub(crate) based_on: Option<String>,
    pub(crate) next_style: Option<String>,
    pub(crate) is_default: bool,
    pub(crate) paragraph_properties: Option<RawParagraphProperties>,
    pub(crate) run_properties: Option<RawRunProperties>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct StyleTable {
    styles: HashMap<String, StyleEntry>,
    pub(crate) doc_defaults_run: Option<RawRunProperties>,
    pub(crate) doc_defaults_paragraph: Option<RawParagraphProperties>,
}

#[allow(dead_code)]
impl StyleTable {
    pub(crate) fn new() -> Self {
        Self {
            styles: HashMap::new(),
            doc_defaults_run: None,
            doc_defaults_paragraph: None,
        }
    }

    pub(crate) fn insert(&mut self, entry: StyleEntry) {
        self.styles.insert(entry.style_id.clone(), entry);
    }

    pub(crate) fn get(&self, id: &str) -> Option<&StyleEntry> {
        self.styles.get(id)
    }

    pub(crate) fn doc_defaults_run_properties(&self) -> Option<&RawRunProperties> {
        self.doc_defaults_run.as_ref()
    }

    pub(crate) fn len(&self) -> usize {
        self.styles.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_entry_construction() {
        let entry = StyleEntry {
            style_id: "Heading1".into(),
            name: "heading 1".into(),
            style_type: StyleType::Paragraph,
            based_on: Some("Normal".into()),
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: None,
        };
        assert_eq!(entry.style_id, "Heading1");
        assert_eq!(entry.name, "heading 1");
        assert_eq!(entry.style_type, StyleType::Paragraph);
        assert_eq!(entry.based_on.as_deref(), Some("Normal"));
        assert!(!entry.is_default);
    }

    #[test]
    fn style_table_get_by_id() {
        let mut table = StyleTable::new();
        table.insert(StyleEntry {
            style_id: "Heading1".into(),
            name: "heading 1".into(),
            style_type: StyleType::Paragraph,
            based_on: Some("Normal".into()),
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: None,
        });
        assert!(table.get("Heading1").is_some());
        assert_eq!(table.get("Heading1").unwrap().name, "heading 1");
    }

    #[test]
    fn style_table_missing_returns_none() {
        let table = StyleTable::new();
        assert!(table.get("NonExistent").is_none());
    }

    #[test]
    fn style_table_doc_defaults() {
        let mut table = StyleTable::new();
        table.doc_defaults_run = Some(RawRunProperties {
            font_size_half_points: Some(24),
            ..Default::default()
        });
        assert_eq!(
            table
                .doc_defaults_run_properties()
                .and_then(|r| r.font_size_half_points),
            Some(24)
        );
    }

    #[test]
    fn style_table_len() {
        let mut table = StyleTable::new();
        assert_eq!(table.len(), 0);
        table.insert(StyleEntry {
            style_id: "Normal".into(),
            name: "Normal".into(),
            style_type: StyleType::Paragraph,
            based_on: None,
            next_style: None,
            is_default: true,
            paragraph_properties: None,
            run_properties: None,
        });
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn style_type_variants() {
        assert_eq!(StyleType::Paragraph, StyleType::Paragraph);
        assert_ne!(StyleType::Paragraph, StyleType::Character);
        assert_ne!(StyleType::Table, StyleType::Numbering);
    }

    #[test]
    fn style_entry_with_run_properties() {
        let entry = StyleEntry {
            style_id: "Strong".into(),
            name: "Strong".into(),
            style_type: StyleType::Character,
            based_on: None,
            next_style: None,
            is_default: false,
            paragraph_properties: None,
            run_properties: Some(RawRunProperties {
                bold: Some(true),
                ..Default::default()
            }),
        };
        assert_eq!(entry.run_properties.as_ref().unwrap().bold, Some(true));
    }
}
