use std::path::PathBuf;

use crate::model::{Record, split_folder};

/// Reads the file asynchronously, then parses it (CPU-bound) on a blocking
/// thread so the UI/async runtime never stalls on large files.
pub async fn load_records(path: PathBuf) -> Result<Vec<Record>, String> {
    let content = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

    tokio::task::spawn_blocking(move || parse_records(&content))
        .await
        .map_err(|e| format!("Parsing task panicked: {e}"))?
}

fn parse_records(content: &[u8]) -> Result<Vec<Record>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'|')
        .has_headers(true)
        .flexible(true)
        .from_reader(content);

    let mut records = Vec::new();
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
    Ok(records)
}
