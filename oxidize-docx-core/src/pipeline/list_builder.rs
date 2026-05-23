use crate::numbering::ListType;
use crate::pipeline::element::DocxElement;

/// A contiguous run of list items, grouped from the flat `DocxElement`
/// stream and nested according to the `level` field of each item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedList {
    pub items: Vec<NestedListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedListItem {
    pub text: String,
    pub list_type: ListType,
    pub display_index: Option<u32>,
    pub children: Vec<NestedListItem>,
}

/// Groups consecutive `DocxElement::ListItem` entries into one
/// `NestedList` each, nesting items by their `level` field. Non-list
/// elements break runs: a Heading or Paragraph between two list items
/// produces two separate `NestedList`s.
#[allow(dead_code)]
pub fn nest_list_items(elements: &[DocxElement]) -> Vec<NestedList> {
    let mut out: Vec<NestedList> = Vec::new();
    let mut current_run: Vec<(u8, NestedListItem)> = Vec::new();
    for elem in elements {
        match elem {
            DocxElement::ListItem {
                text,
                level,
                list_type,
                display_index,
            } => {
                current_run.push((
                    *level,
                    NestedListItem {
                        text: text.clone(),
                        list_type: list_type.clone(),
                        display_index: *display_index,
                        children: vec![],
                    },
                ));
            }
            _ => {
                if !current_run.is_empty() {
                    out.push(NestedList {
                        items: build_run(std::mem::take(&mut current_run)),
                    });
                }
            }
        }
    }
    if !current_run.is_empty() {
        out.push(NestedList {
            items: build_run(current_run),
        });
    }
    out
}

/// Builds a parent/child tree from a flat list of (level, leaf-node)
/// pairs in document order, using a stack of indices to keep borrow
/// checking honest. Items at the same level become siblings; items at
/// deeper levels become children of the most recent shallower-level
/// item still on the stack.
fn build_run(items: Vec<(u8, NestedListItem)>) -> Vec<NestedListItem> {
    struct FlatNode {
        node: NestedListItem,
        level: u8,
        parent: Option<usize>,
    }

    let mut flat: Vec<FlatNode> = Vec::with_capacity(items.len());
    let mut stack: Vec<usize> = Vec::new();

    for (level, leaf) in items {
        while let Some(&top) = stack.last() {
            if flat[top].level < level {
                break;
            }
            stack.pop();
        }
        let parent = stack.last().copied();
        let idx = flat.len();
        flat.push(FlatNode {
            node: leaf,
            level,
            parent,
        });
        stack.push(idx);
    }

    let n = flat.len();
    let mut children_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots: Vec<usize> = Vec::new();
    for (i, node) in flat.iter().enumerate() {
        match node.parent {
            Some(p) => children_of[p].push(i),
            None => roots.push(i),
        }
    }

    fn make_node(idx: usize, flat: &[FlatNode], children_of: &[Vec<usize>]) -> NestedListItem {
        let mut node = flat[idx].node.clone();
        node.children = children_of[idx]
            .iter()
            .map(|&ci| make_node(ci, flat, children_of))
            .collect();
        node
    }

    roots
        .iter()
        .map(|&r| make_node(r, &flat, &children_of))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: &str, level: u8, idx: u32) -> DocxElement {
        DocxElement::ListItem {
            text: text.into(),
            level,
            list_type: ListType::Decimal,
            display_index: Some(idx),
        }
    }

    fn leaf(text: &str, idx: u32) -> NestedListItem {
        NestedListItem {
            text: text.into(),
            list_type: ListType::Decimal,
            display_index: Some(idx),
            children: vec![],
        }
    }

    #[test]
    fn list_items_separated_by_heading_produce_two_distinct_nested_lists() {
        let elements = vec![
            item("A", 0, 1),
            DocxElement::Heading {
                level: 1,
                text: "H".into(),
            },
            item("B", 0, 1),
        ];
        let nested = nest_list_items(&elements);
        assert_eq!(
            nested,
            vec![
                NestedList {
                    items: vec![leaf("A", 1)]
                },
                NestedList {
                    items: vec![leaf("B", 1)]
                },
            ]
        );
    }

    #[test]
    fn nested_levels_0_1_0_1_2_build_correct_parent_child_tree() {
        // [A(0), A.1(1), B(0), B.1(1), B.1.a(2)]
        // Expected tree:
        //   - A
        //     - A.1
        //   - B
        //     - B.1
        //       - B.1.a
        let elements = vec![
            item("A", 0, 1),
            item("A.1", 1, 1),
            item("B", 0, 2),
            item("B.1", 1, 1),
            item("B.1.a", 2, 1),
        ];
        let nested = nest_list_items(&elements);
        assert_eq!(
            nested,
            vec![NestedList {
                items: vec![
                    NestedListItem {
                        text: "A".into(),
                        list_type: ListType::Decimal,
                        display_index: Some(1),
                        children: vec![leaf("A.1", 1)],
                    },
                    NestedListItem {
                        text: "B".into(),
                        list_type: ListType::Decimal,
                        display_index: Some(2),
                        children: vec![NestedListItem {
                            text: "B.1".into(),
                            list_type: ListType::Decimal,
                            display_index: Some(1),
                            children: vec![leaf("B.1.a", 1)],
                        }],
                    },
                ],
            }]
        );
    }

    #[test]
    fn two_same_level_items_are_grouped_into_one_nested_list_as_siblings() {
        let elements = vec![item("A", 0, 1), item("B", 0, 2)];
        let nested = nest_list_items(&elements);
        assert_eq!(
            nested,
            vec![NestedList {
                items: vec![leaf("A", 1), leaf("B", 2)],
            }]
        );
    }

    #[test]
    fn single_list_item_yields_one_nested_list_with_one_leaf_item() {
        let elements = vec![DocxElement::ListItem {
            text: "A".into(),
            level: 0,
            list_type: ListType::Decimal,
            display_index: Some(1),
        }];
        let nested = nest_list_items(&elements);
        assert_eq!(
            nested,
            vec![NestedList {
                items: vec![NestedListItem {
                    text: "A".into(),
                    list_type: ListType::Decimal,
                    display_index: Some(1),
                    children: vec![],
                }]
            }]
        );
    }
}
