use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::model::{Record, split_folder};

/// Everything the UI needs after a load: the records plus indices/lookups
/// that are expensive to build (O(n) over up to millions of rows), so they
/// are computed once here on a background thread instead of every frame.
pub struct LoadedData {
    pub records: Vec<Record>,
    pub folder_index: HashMap<Arc<str>, Vec<usize>>,
    pub identity_index: HashMap<Arc<str>, Vec<usize>>,
    pub other_index: HashMap<Arc<str>, Vec<usize>>,
    pub unique_folders: Vec<String>,
    pub unique_identities: Vec<String>,
    pub unique_others: Vec<String>,
}

/// Reads the file asynchronously, then parses and indexes it (CPU-bound) on
/// a blocking thread so the UI/async runtime never stalls, even for
/// multi-million-row files. Rows whose Identity exactly matches an entry in
/// `excluded_identities` are dropped entirely (e.g. noisy built-in
/// principals like `BUILTIN\Administrators`). `split_depth` controls how
/// many leading backslashes go into the Folder column before the rest
/// falls into Other; if `auto_split` is set, that's used only as a floor
/// and the split is extended automatically past it (see
/// `auto_split_folder`).
pub async fn load_records(
    path: PathBuf,
    excluded_identities: HashSet<String>,
    split_depth: usize,
    auto_split: bool,
) -> Result<LoadedData, String> {
    let content = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    tokio::task::spawn_blocking(move || {
        parse_and_index(&content, &excluded_identities, split_depth, auto_split)
    })
    .await
    .map_err(|e| format!("Parsing task panicked: {e}"))?
}

/// De-duplicates repeated field values into a single `Arc<str>` allocation.
/// ACL exports are extremely repetitive (the same folder path repeated once
/// per grantee, a handful of distinct Rights/AccessControl values), so this
/// avoids millions of redundant string allocations and shrinks memory a lot.
struct Interner {
    pool: HashMap<Box<str>, Arc<str>>,
}

impl Interner {
    fn new(capacity: usize) -> Self {
        Self {
            pool: HashMap::with_capacity(capacity),
        }
    }

    fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(existing) = self.pool.get(s) {
            return existing.clone();
        }
        let arc: Arc<str> = Arc::from(s);
        self.pool.insert(Box::from(s), arc.clone());
        arc
    }
}

fn parse_and_index(
    content: &[u8],
    excluded_identities: &HashSet<String>,
    split_depth: usize,
    auto_split: bool,
) -> Result<LoadedData, String> {
    // Automatic mode needs to know the whole path structure before it can
    // decide where any single row should split, so it does one extra pass
    // up front to build a trie of every row's full Folder path.
    let trie = if auto_split {
        Some(build_split_trie(content, excluded_identities)?)
    } else {
        None
    };

    // Rough capacity hint from newline count, so the record Vec doesn't
    // have to repeatedly reallocate/copy while growing to millions of rows.
    let approx_rows = content.iter().filter(|&&b| b == b'\n').count().max(16);

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'|')
        .has_headers(true)
        .flexible(true)
        .from_reader(content);

    let mut records = Vec::with_capacity(approx_rows);
    // Most exports have far fewer distinct values than rows; this hint just
    // avoids a few early pool rehashes, it doesn't need to be exact.
    let mut interner = Interner::new((approx_rows / 20).max(64));

    for result in reader.records() {
        let row = result.map_err(|e| format!("CSV parse error: {e}"))?;
        let raw_folder = row.get(0).unwrap_or("").trim();
        let identity_raw = row.get(1).unwrap_or("").trim();
        if excluded_identities.contains(identity_raw) {
            continue;
        }
        let identity = interner.intern(identity_raw);
        let rights = interner.intern(row.get(2).unwrap_or("").trim());
        let access_control = interner.intern(row.get(3).unwrap_or("").trim());
        let inherited = interner.intern(row.get(4).unwrap_or("").trim());
        let (folder_raw, other_raw) = match &trie {
            Some((trie, total_identities)) => {
                auto_split_folder(raw_folder, split_depth, trie, *total_identities)
            }
            None => split_folder(raw_folder, split_depth),
        };
        let folder = interner.intern(folder_raw);
        let other = interner.intern(other_raw);
        records.push(Record {
            folder,
            other,
            identity,
            rights,
            access_control,
            inherited,
        });
    }

    // Most CSVs like this have far fewer unique folders/identities/others
    // than rows; a modest capacity hint avoids repeated HashMap rehashing
    // while still being cheap to over/under-estimate.
    let index_capacity = (records.len() / 4).max(16);
    let mut folder_index: HashMap<Arc<str>, Vec<usize>> = HashMap::with_capacity(index_capacity);
    let mut identity_index: HashMap<Arc<str>, Vec<usize>> = HashMap::with_capacity(index_capacity);
    let mut other_index: HashMap<Arc<str>, Vec<usize>> = HashMap::with_capacity(index_capacity);
    for (i, r) in records.iter().enumerate() {
        if !r.folder.is_empty() {
            // Arc clone: just a refcount bump, not a string copy.
            folder_index.entry(r.folder.clone()).or_default().push(i);
        }
        if !r.identity.is_empty() {
            identity_index
                .entry(r.identity.clone())
                .or_default()
                .push(i);
        }
        if !r.other.is_empty() {
            other_index.entry(r.other.clone()).or_default().push(i);
        }
    }

    let mut unique_folders: Vec<String> = folder_index.keys().map(|k| k.to_string()).collect();
    unique_folders.sort_unstable();
    let mut unique_identities: Vec<String> = identity_index.keys().map(|k| k.to_string()).collect();
    unique_identities.sort_unstable();
    let mut unique_others: Vec<String> = other_index.keys().map(|k| k.to_string()).collect();
    unique_others.sort_unstable();

    Ok(LoadedData {
        records,
        folder_index,
        identity_index,
        other_index,
        unique_folders,
        unique_identities,
        unique_others,
    })
}

/// Trie of every row's full Folder-column path (segments split on `\`).
/// Each node tracks which identities have a grant somewhere at or below
/// it, as small integer ids — used to tell whether a prefix is still
/// "shared by every user in the data" (uninteresting, keep going deeper)
/// or has narrowed down to a proper subset (a real access boundary).
#[derive(Default)]
struct SplitTrieNode {
    children: HashMap<String, SplitTrieNode>,
    identities: HashSet<u32>,
}

impl SplitTrieNode {
    fn insert(&mut self, segments: &[&str], identity_id: u32) {
        self.identities.insert(identity_id);
        if segments.is_empty() {
            return;
        }
        let child = self.children.entry(segments[0].to_string()).or_default();
        child.insert(&segments[1..], identity_id);
    }

    /// Whether this node's grants cover every identity in the dataset.
    fn is_universal(&self, total_identities: usize) -> bool {
        self.identities.len() >= total_identities
    }
}

/// First pass over the data (automatic-split mode only). Mirrors the same
/// identity exclusion as the main pass, so the trie reflects exactly what
/// will actually be loaded. Returns the trie plus the total number of
/// distinct identities seen, which is what "shared by all users" is
/// measured against.
fn build_split_trie(
    content: &[u8],
    excluded_identities: &HashSet<String>,
) -> Result<(SplitTrieNode, usize), String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'|')
        .has_headers(true)
        .flexible(true)
        .from_reader(content);

    let mut root = SplitTrieNode::default();
    let mut identity_ids: HashMap<String, u32> = HashMap::new();

    for result in reader.records() {
        let row = result.map_err(|e| format!("CSV parse error: {e}"))?;
        let identity = row.get(1).unwrap_or("").trim();
        if excluded_identities.contains(identity) {
            continue;
        }
        let next_id = identity_ids.len() as u32;
        let id = *identity_ids.entry(identity.to_string()).or_insert(next_id);

        let raw_folder = row.get(0).unwrap_or("").trim();
        let segments: Vec<&str> = raw_folder.split('\\').filter(|s| !s.is_empty()).collect();
        root.insert(&segments, id);
    }
    let total_identities = identity_ids.len();
    Ok((root, total_identities))
}

/// Like `split_folder`, but instead of stopping at a fixed backslash count,
/// keeps extending the Folder portion past `floor_depth` for as long as the
/// accumulated prefix is still granted to literally every identity in the
/// data (i.e. not yet distinguishing). It stops the moment a prefix's
/// grants stop covering everyone — that's the boundary where access
/// actually starts differing between users, which is what makes a folder
/// worth splitting out on its own.
fn auto_split_folder<'a>(
    path: &'a str,
    floor_depth: usize,
    trie: &SplitTrieNode,
    total_identities: usize,
) -> (&'a str, &'a str) {
    let mut node = trie;
    let mut count = 0usize;
    let mut cut = 0usize;
    let mut seg_start = 0usize;

    for (i, c) in path.char_indices() {
        if c != '\\' {
            continue;
        }
        let segment = &path[seg_start..i];
        count += 1;
        seg_start = i + 1;

        if segment.is_empty() {
            // Consecutive backslashes — most commonly the leading `\\` of
            // a UNC path. There's no trie node for "", so just pass
            // through rather than doing a lookup that would always fail.
            cut = i + 1;
            continue;
        }

        if count > floor_depth && !node.is_universal(total_identities) {
            // `node` (the prefix already accepted as Folder) already
            // isn't shared by everyone — that boundary was crossed on a
            // previous step, so stop rather than fragment further.
            break;
        }

        let Some(child) = node.children.get(segment) else {
            // Shouldn't happen — the trie was built from this same data —
            // but bail out to the fixed-depth cut rather than panic.
            break;
        };
        node = child;
        cut = i + 1;
    }

    (&path[..cut], &path[cut..])
}
