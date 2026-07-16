use eframe::egui;
use egui_extras::{Column, TableBuilder};

const COLUMNS: [&str; 6] = [
    "Folder",
    "Other",
    "Identity",
    "Rights",
    "AccessControl",
    "Inherited",
];

#[derive(Clone)]
struct Record {
    folder: String,
    other: String,
    identity: String,
    rights: String,
    access_control: String,
    inherited: String,
}

impl Record {
    fn field(&self, col: usize) -> &str {
        match col {
            0 => &self.folder,
            1 => &self.other,
            2 => &self.identity,
            3 => &self.rights,
            4 => &self.access_control,
            _ => &self.inherited,
        }
    }
}

/// Splits `path` right after the 4th backslash. Everything up to and
/// including the 4th `\` becomes `folder`, the remainder becomes `other`.
/// If there are fewer than 4 backslashes, `folder` is the whole string
/// and `other` is empty.
fn split_folder(path: &str) -> (String, String) {
    let mut count = 0;
    for (i, c) in path.char_indices() {
        if c == '\\' {
            count += 1;
            if count == 4 {
                let folder = &path[..=i];
                let other = &path[i + 1..];
                return (folder.to_string(), other.to_string());
            }
        }
    }
    (path.to_string(), String::new())
}

fn load_records(path: &std::path::Path) -> Result<Vec<Record>, Box<dyn std::error::Error>> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'|')
        .has_headers(true)
        .flexible(true)
        .from_path(path)?;

    let mut records = Vec::new();
    for result in reader.records() {
        let row = result?;
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

struct FolderAclApp {
    records: Vec<Record>,
    filtered: Vec<usize>,
    query: String,
    sort_col: usize,
    sort_desc: bool,
    loaded_path: Option<String>,
    error: Option<String>,
}

impl Default for FolderAclApp {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            filtered: Vec::new(),
            query: String::new(),
            sort_col: 0,
            sort_desc: false,
            loaded_path: None,
            error: None,
        }
    }
}

impl FolderAclApp {
    fn load_path(&mut self, path: &std::path::Path) {
        match load_records(path) {
            Ok(records) => {
                self.records = records;
                self.loaded_path = Some(path.display().to_string());
                self.error = None;
                self.apply_filter_and_sort();
            }
            Err(e) => self.error = Some(format!("Failed to load {}: {e}", path.display())),
        }
    }

    fn apply_filter_and_sort(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = (0..self.records.len())
            .filter(|&i| {
                if q.is_empty() {
                    true
                } else {
                    let r = &self.records[i];
                    r.folder.to_lowercase().contains(&q)
                        || r.other.to_lowercase().contains(&q)
                        || r.identity.to_lowercase().contains(&q)
                        || r.rights.to_lowercase().contains(&q)
                        || r.access_control.to_lowercase().contains(&q)
                        || r.inherited.to_lowercase().contains(&q)
                }
            })
            .collect();

        let col = self.sort_col;
        let desc = self.sort_desc;
        let records = &self.records;
        self.filtered.sort_by(|&a, &b| {
            let ord = records[a].field(col).cmp(records[b].field(col));
            if desc { ord.reverse() } else { ord }
        });
    }

    fn set_sort(&mut self, col: usize) {
        if self.sort_col == col {
            self.sort_desc = !self.sort_desc;
        } else {
            self.sort_col = col;
            self.sort_desc = false;
        }
        self.apply_filter_and_sort();
    }
}

impl eframe::App for FolderAclApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Open CSV...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .pick_file()
                    {
                        self.load_path(&path);
                    }
                }

                ui.separator();
                ui.label("Search:");
                if ui.text_edit_singleline(&mut self.query).changed() {
                    self.apply_filter_and_sort();
                }
                if ui.button("Clear").clicked() {
                    self.query.clear();
                    self.apply_filter_and_sort();
                }

                ui.separator();
                if let Some(path) = &self.loaded_path {
                    ui.label(format!("File: {path}"));
                }
                ui.label(format!(
                    "Rows: {} / {}",
                    self.filtered.len(),
                    self.records.len()
                ));
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, err);
            }

            if self.records.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a pipe-delimited CSV (Folder|Identity|Rights|AccessControl|Inherited) to begin.");
                });
                return;
            }

            let mut sort_clicked: Option<usize> = None;

            TableBuilder::new(ui)
                .id_salt("folder_acl_table")
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .columns(Column::auto().resizable(true).at_least(80.0), COLUMNS.len())
                .min_scrolled_height(0.0)
                .header(24.0, |mut header| {
                    for (i, name) in COLUMNS.iter().enumerate() {
                        header.col(|ui| {
                            let label = if i == self.sort_col {
                                format!("{name} {}", if self.sort_desc { "▼" } else { "▲" })
                            } else {
                                name.to_string()
                            };
                            if ui.button(label).clicked() {
                                sort_clicked = Some(i);
                            }
                        });
                    }
                })
                .body(|body| {
                    let row_height = 20.0;
                    let filtered = &self.filtered;
                    let records = &self.records;
                    body.rows(row_height, filtered.len(), |mut row| {
                        let idx = filtered[row.index()];
                        let r = &records[idx];
                        for col in 0..COLUMNS.len() {
                            row.col(|ui| {
                                ui.label(r.field(col));
                            });
                        }
                    });
                });

            if let Some(col) = sort_clicked {
                self.set_sort(col);
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let cli_path = std::env::args().nth(1);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 650.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Folder ACL Viewer",
        options,
        Box::new(move |_cc| {
            let mut app = FolderAclApp::default();
            if let Some(path) = &cli_path {
                app.load_path(std::path::Path::new(path));
            }
            Ok(Box::new(app))
        }),
    )
}
