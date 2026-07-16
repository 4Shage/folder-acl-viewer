use std::collections::HashMap;
use std::path::PathBuf;

use crate::model::{Record, split_folder};

/// Everything the UI needs after a load: the records plus indices/lookups
/// that are expensive to build (O(n) over up to millions of rows), so they
/// are computed once here on a background thread instead of every frame.
pub struct LoadedData {
    pub records: Vec<Record>,
    pub folder_index: HashMap<String, Vec<usize>>,
    pub identity_index: HashMap<String, Vec<usize>>,
    pub unique_folders: Vec<String>,
    pub unique_identities: Vec<String>,
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

fn parse_and_index(content: &[u8]) -> Result<LoadedData, String> {
    // Rough capacity hint from newline count, so the record Vec doesn't
    // have to repeatedly reallocate/copy while growing to millions of rows.
    let approx_rows = content.iter().filter(|&&b| b == b'\n').count().max(16);
    println!("{}", approx_rows);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'|')
        .has_headers(true)
        .flexible(true)
        .from_reader(content);

    let mut records = Vec::with_capacity(approx_rows);
    for result in reader.records() {
        let row = result.map_err(|e| format!("CSV parse error: {e}"))?;
        let raw_folder = row.get(0).unwrap_or("").trim();
        let identity = row.get(1).unwrap_or("").trim().to_string();
        let rights = row.get(2).unwrap_or("").trim().to_string();
        let access_control = row.get(3).unwrap_or("").trim().to_string();
        let inherited = row.get(4).unwrap_or("").trim().to_string();
        let (folder, other) = split_folder(raw_folder);
        records.push(Record {
            folder,
            other,
            identity,
            rights,
            access_control,
            inherited,
        });
    }

    let mut folder_index: HashMap<String, Vec<usize>> = HashMap::new();
    let mut identity_index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, r) in records.iter().enumerate() {
        if !r.folder.is_empty() {
            folder_index.entry(r.folder.clone()).or_default().push(i);
        }
        if !r.identity.is_empty() {
            identity_index
                .entry(r.identity.clone())
                .or_default()
                .push(i);
        }
    }

    let mut unique_folders: Vec<String> = folder_index.keys().cloned().collect();
    unique_folders.sort();
    let mut unique_identities: Vec<String> = identity_index.keys().cloned().collect();
    unique_identities.sort();

    Ok(LoadedData {
        records,
        folder_index,
        identity_index,
        unique_folders,
        unique_identities,
    })
}
