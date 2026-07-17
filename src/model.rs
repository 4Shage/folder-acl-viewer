use std::collections::BTreeMap;
use std::sync::Arc;

pub const COLUMNS: [&str; 6] = [
    "Folder",
    "Other",
    "Identity",
    "Rights",
    "AccessControl",
    "Inherited",
];

/// Interned string: cheap to clone (refcount bump, no allocation or copy)
/// and much smaller in memory than storing the same text over and over.
/// ACL exports are extremely repetitive — a handful of Rights/AccessControl
/// values and a folder path repeated once per grantee — so interning is a
/// large win on both memory and clone cost for multi-million-row files.
pub type IStr = Arc<str>;

#[derive(Clone)]
pub struct Record {
    pub folder: IStr,
    pub other: IStr,
    pub identity: IStr,
    pub rights: IStr,
    pub access_control: IStr,
    pub inherited: IStr,
}

impl Record {
    pub fn field(&self, col: usize) -> &str {
        match col {
            0 => &self.folder,
            1 => &self.other,
            2 => &self.identity,
            3 => &self.rights,
            4 => &self.access_control,
            _ => &self.inherited,
        }
    }

    pub fn full_path(&self) -> String {
        format!("{}{}", self.folder, self.other)
    }
}

/// Splits `path` right after the 4th backslash. Everything up to and
/// including the 4th `\` becomes `folder`, the remainder becomes `other`.
/// If there are fewer than 4 backslashes, `folder` is the whole string
/// and `other` is empty.
///
/// Borrows from `path` instead of allocating, so the caller can decide
/// whether/how to intern the pieces.
pub fn split_folder(path: &str) -> (&str, &str) {
    let mut count = 0;
    for (i, c) in path.char_indices() {
        if c == '\\' {
            count += 1;
            if count == 6 {
                return (&path[..=i], &path[i + 1..]);
            }
        }
    }
    (path, "")
}

/// A node in a folder tree. `entries` holds indices into the record list
/// for permissions that apply exactly at this node's path.
#[derive(Default)]
pub struct TreeNode {
    pub children: BTreeMap<String, TreeNode>,
    pub entries: Vec<usize>,
}

impl TreeNode {
    fn insert(&mut self, segments: &[&str], idx: usize) {
        if segments.is_empty() {
            self.entries.push(idx);
            return;
        }
        let child = self.children.entry(segments[0].to_string()).or_default();
        child.insert(&segments[1..], idx);
    }
}

/// Builds a tree from the full reconstructed path (`folder` + `other`) of
/// the given record indices. Used for the per-user view, since a user can
/// show up under several folder roots.
pub fn build_path_tree(records: &[Record], indices: &[usize]) -> TreeNode {
    let mut root = TreeNode::default();
    for &idx in indices {
        let full = records[idx].full_path();
        let segments: Vec<&str> = full.split('\\').filter(|s| !s.is_empty()).collect();
        root.insert(&segments, idx);
    }
    root
}

/// Builds a tree from only the `other` sub-path of the given record
/// indices. Used for the per-folder view, where the folder root is already
/// implied by the selection.
pub fn build_other_tree(records: &[Record], indices: &[usize]) -> TreeNode {
    let mut root = TreeNode::default();
    for &idx in indices {
        let other = &records[idx].other;
        let segments: Vec<&str> = other.split('\\').filter(|s| !s.is_empty()).collect();
        root.insert(&segments, idx);
    }
    root
}

/// Tracks which folder nodes differ from the tree's default open/closed
/// state. Only exceptions are stored, so switching a whole (possibly huge)
/// tree between "all expanded" and "all collapsed" is O(1) instead of
/// having to touch every node.
#[derive(Default)]
pub struct ExpansionState {
    pub default_open: bool,
    toggled: std::collections::HashSet<String>,
}

impl ExpansionState {
    pub fn new(default_open: bool) -> Self {
        Self {
            default_open,
            toggled: std::collections::HashSet::new(),
        }
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.default_open != self.toggled.contains(id)
    }

    pub fn toggle(&mut self, id: String) {
        if !self.toggled.remove(&id) {
            self.toggled.insert(id);
        }
    }
}

/// One row of a flattened tree, ready for virtualized rendering (only the
/// rows actually visible in the scroll viewport need to be turned into
/// widgets each frame — see `ScrollArea::show_rows` in app.rs).
pub enum FlatRow {
    Folder {
        id: String,
        name: String,
        depth: usize,
        expanded: bool,
        has_children: bool,
    },
    Entry {
        record_idx: usize,
        depth: usize,
    },
}

/// Flattens a `TreeNode` into a linear list of rows, honoring `expansion`.
/// Collapsed subtrees are skipped entirely, so this is proportional to the
/// number of *visible* (open) nodes, not the whole tree.
pub fn flatten_tree(tree: &TreeNode, expansion: &ExpansionState) -> Vec<FlatRow> {
    let mut out = Vec::new();
    flatten_node(tree, "", 0, expansion, &mut out);
    out
}

fn flatten_node(
    node: &TreeNode,
    id_prefix: &str,
    depth: usize,
    expansion: &ExpansionState,
    out: &mut Vec<FlatRow>,
) {
    for &idx in &node.entries {
        out.push(FlatRow::Entry {
            record_idx: idx,
            depth,
        });
    }
    for (name, child) in &node.children {
        let id = if id_prefix.is_empty() {
            name.clone()
        } else {
            format!("{id_prefix}/{name}")
        };
        let expanded = expansion.is_open(&id);
        let has_children = !child.children.is_empty() || !child.entries.is_empty();
        out.push(FlatRow::Folder {
            id: id.clone(),
            name: name.clone(),
            depth,
            expanded,
            has_children,
        });
        if expanded {
            flatten_node(child, &id, depth + 1, expansion, out);
        }
    }
}
