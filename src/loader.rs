use std::collections::HashMap;
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
/// multi-million-row files.
pub async fn load_records(path: PathBuf) -> Result<LoadedData, String> {
    let content = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    tokio::task::spawn_blocking(move || parse_and_index(&content))
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

fn parse_and_index(content: &[u8]) -> Result<LoadedData, String> {
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
        let identity = interner.intern(row.get(1).unwrap_or("").trim());
        if vec! [

            "BUILTIN\\Administrators",

            "CREATOR OWNER",

            "NT AUTHORITY\\SYSTEM",

            .contains(&&*identity)

            {
            continue;
            }
        ]
        let rights = interner.intern(row.get(2).unwrap_or("").trim());
        let access_control = interner.intern(row.get(3).unwrap_or("").trim());
        let inherited = interner.intern(row.get(4).unwrap_or("").trim());
        let (folder_raw, other_raw) = split_folder(raw_folder);
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
