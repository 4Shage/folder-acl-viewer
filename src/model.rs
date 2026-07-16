use std::collections::BTreeMap;
use std::collections::BTreeSet;

pub const COLUMNS: [&str; 6] = [
    "Folder",
    "Other",
    "Identity",
    "Rights",
    "AccessControl",
    "Inherited",
];

#[derive(Clone)]
pub struct Record {
    pub folder: String,
    pub other: String,
    pub identity: String,
    pub rights: String,
    pub access_control: String,
    pub inherited: String,
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
pub fn split_folder(path: &str) -> (String, String) {
    let mut count = 0;
    for (i, c) in path.char_indices() {
        if c == '\\' {
            count += 1;
            if count == 4 {
                return (path[..=i].to_string(), path[i + 1..].to_string());
            }
        }
    }
    (path.to_string(), String::new())
}

pub fn unique_folders(records: &[Record]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for r in records {
        if !r.folder.is_empty() {
            set.insert(r.folder.clone());
        }
    }
    set.into_iter().collect()
}

pub fn unique_identities(records: &[Record]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for r in records {
        if !r.identity.is_empty() {
            set.insert(r.identity.clone());
        }
    }
    set.into_iter().collect()
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
