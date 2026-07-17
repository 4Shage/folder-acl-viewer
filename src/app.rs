use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use tokio::runtime::Handle;

use crate::loader::{self, LoadedData};
use crate::model::{
    COLUMNS, ExpansionState, FlatRow, Record, TreeNode, build_other_tree, build_path_tree,
    flatten_tree,
};

#[derive(PartialEq, Clone, Copy)]
enum SearchBy {
    Folder,
    User,
    Other,
}

enum Selection {
    None,
    Folder {
        name: String,
        tree: TreeNode,
        expansion: ExpansionState,
        flat: Vec<FlatRow>,
    },
    User {
        name: String,
        tree: TreeNode,
        expansion: ExpansionState,
        flat: Vec<FlatRow>,
    },
    Other {
        name: String,
        tree: TreeNode,
        expansion: ExpansionState,
        flat: Vec<FlatRow>,
    },
}

pub struct FolderAclApp {
    records: Vec<Record>,
    folder_index: HashMap<Arc<str>, Vec<usize>>,
    identity_index: HashMap<Arc<str>, Vec<usize>>,
    other_index: HashMap<Arc<str>, Vec<usize>>,
    unique_folders: Vec<String>,
    unique_identities: Vec<String>,
    unique_others: Vec<String>,
    // Lower-cased mirrors of the three lists above, built once at load time
    // so filtering on every keystroke doesn't re-lowercase (and
    // re-allocate) every name each time.
    unique_folders_lower: Vec<String>,
    unique_identities_lower: Vec<String>,
    unique_others_lower: Vec<String>,

    filtered_table: Vec<usize>,
    table_sort_col: usize,
    table_sort_desc: bool,

    search_by: SearchBy,
    sidebar_query: String,
    // Indices into unique_folders / unique_identities (depending on
    // search_by) matching the current query — avoids cloning the whole
    // name list into a fresh Vec<String> on every keystroke.
    sidebar_cache: Vec<usize>,
    selection: Selection,

    loaded_path: Option<String>,
    error: Option<String>,
    loading: bool,

    rt: Handle,
    result_tx: Sender<Result<(PathBuf, LoadedData), String>>,
    result_rx: Receiver<Result<(PathBuf, LoadedData), String>>,
}

impl FolderAclApp {
    pub fn new(rt: Handle) -> Self {
        let (result_tx, result_rx) = channel();
        Self {
            records: Vec::new(),
            folder_index: HashMap::new(),
            identity_index: HashMap::new(),
            other_index: HashMap::new(),
            unique_folders: Vec::new(),
            unique_identities: Vec::new(),
            unique_others: Vec::new(),
            unique_folders_lower: Vec::new(),
            unique_identities_lower: Vec::new(),
            unique_others_lower: Vec::new(),
            filtered_table: Vec::new(),
            table_sort_col: 0,
            table_sort_desc: false,
            search_by: SearchBy::Folder,
            sidebar_query: String::new(),
            sidebar_cache: Vec::new(),
            selection: Selection::None,
            loaded_path: None,
            error: None,
            loading: false,
            rt,
            result_tx,
            result_rx,
        }
    }

    pub fn request_load(&mut self, path: PathBuf) {
        self.loading = true;
        self.error = None;
        let tx = self.result_tx.clone();
        self.rt.spawn(async move {
            let res = loader::load_records(path.clone()).await;
            let _ = tx.send(res.map(|data| (path, data)));
        });
    }

    fn on_loaded(&mut self, path: PathBuf, data: LoadedData) {
        self.records = data.records;
        self.folder_index = data.folder_index;
        self.identity_index = data.identity_index;
        self.other_index = data.other_index;
        self.unique_folders_lower = data
            .unique_folders
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        self.unique_identities_lower = data
            .unique_identities
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        self.unique_others_lower = data
            .unique_others
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        self.unique_folders = data.unique_folders;
        self.unique_identities = data.unique_identities;
        self.unique_others = data.unique_others;

        self.loaded_path = Some(path.display().to_string());
        self.selection = Selection::None;
        self.table_sort_col = 0;
        self.table_sort_desc = false;
        self.filtered_table = (0..self.records.len()).collect();
        self.refresh_sidebar_cache();
    }

    fn refresh_sidebar_cache(&mut self) {
        let lower = match self.search_by {
            SearchBy::Folder => &self.unique_folders_lower,
            SearchBy::User => &self.unique_identities_lower,
            SearchBy::Other => &self.unique_others_lower,
        };
        self.sidebar_cache = if self.sidebar_query.is_empty() {
            (0..lower.len()).collect()
        } else {
            let q = self.sidebar_query.to_lowercase();
            (0..lower.len())
                .filter(|&i| lower[i].contains(&q))
                .collect()
        };
    }

    /// Looks up the pre-built index (O(1)) instead of scanning all records,
    /// so selecting a folder/user stays instant even at millions of rows.
    fn select_index(&mut self, idx: usize) {
        match self.search_by {
            SearchBy::Folder => {
                let name = self.unique_folders[idx].clone();
                let indices = self
                    .folder_index
                    .get(name.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let tree = build_other_tree(&self.records, indices);
                // Folder view starts collapsed: a share root can fan out
                // very wide, so we don't want to build/lay out thousands of
                // rows up front.
                let expansion = ExpansionState::new(true);
                let flat = flatten_tree(&tree, &expansion);
                self.selection = Selection::Folder {
                    name,
                    tree,
                    expansion,
                    flat,
                };
            }
            SearchBy::User => {
                let name = self.unique_identities[idx].clone();
                let indices = self
                    .identity_index
                    .get(name.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let tree = build_path_tree(&self.records, indices);
                // User view starts fully expanded so the whole access map
                // is visible at a glance.
                let expansion = ExpansionState::new(true);
                let flat = flatten_tree(&tree, &expansion);
                self.selection = Selection::User {
                    name,
                    tree,
                    expansion,
                    flat,
                };
            }
            SearchBy::Other => {
                let name = self.unique_others[idx].clone();
                let indices = self
                    .other_index
                    .get(name.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                // A given sub-path can recur under several different folder
                // roots, so use the full-path tree (like the user view) and
                // start it collapsed, since that fan-out can be wide.
                let tree = build_path_tree(&self.records, indices);
                let expansion = ExpansionState::new(true);
                let flat = flatten_tree(&tree, &expansion);
                self.selection = Selection::Other {
                    name,
                    tree,
                    expansion,
                    flat,
                };
            }
        }
    }
}

impl eframe::App for FolderAclApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(result) = self.result_rx.try_recv() {
            self.loading = false;
            match result {
                Ok((path, data)) => self.on_loaded(path, data),
                Err(e) => self.error = Some(e),
            }
        }
        if self.loading {
            ui.ctx().request_repaint();
        }

        self.top_panel(ui);
        self.sidebar_panel(ui);
        self.central_panel(ui);
    }
}

impl FolderAclApp {
    fn top_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("📁 Folder ACL Viewer");
                ui.separator();
                if ui.button("📂 Open CSV...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .pick_file()
                    {
                        self.request_load(path);
                    }
                }
                if self.loading {
                    ui.add(egui::Spinner::new());
                    ui.label("Loading...");
                }
                ui.separator();
                if let Some(path) = &self.loaded_path {
                    ui.label(egui::RichText::new(path).weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{} rows", self.records.len()));
                });
            });
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), format!("⚠ {err}"));
            }
            ui.add_space(6.0);
        });
    }

    fn sidebar_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(270.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(self.search_by == SearchBy::Folder, "📁 Folders")
                        .clicked()
                        && self.search_by != SearchBy::Folder
                    {
                        self.search_by = SearchBy::Folder;
                        self.sidebar_query.clear();
                        self.refresh_sidebar_cache();
                    }
                    if ui
                        .selectable_label(self.search_by == SearchBy::User, "👤 Users")
                        .clicked()
                        && self.search_by != SearchBy::User
                    {
                        self.search_by = SearchBy::User;
                        self.sidebar_query.clear();
                        self.refresh_sidebar_cache();
                    }
                    if ui
                        .selectable_label(self.search_by == SearchBy::Other, "🗂 Other")
                        .clicked()
                        && self.search_by != SearchBy::Other
                    {
                        self.search_by = SearchBy::Other;
                        self.sidebar_query.clear();
                        self.refresh_sidebar_cache();
                    }
                });
                ui.add_space(4.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.sidebar_query)
                        .hint_text(match self.search_by {
                            SearchBy::Folder => "🔍 Search folders...",
                            SearchBy::User => "🔍 Search users...",
                            SearchBy::Other => "🔍 Search other paths...",
                        })
                        .desired_width(f32::INFINITY),
                );
                if edit.changed() {
                    self.refresh_sidebar_cache();
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("{} matches", self.sidebar_cache.len())).weak(),
                );
                ui.add_space(2.0);
                ui.separator();

                let icon = match self.search_by {
                    SearchBy::Folder => "📁",
                    SearchBy::User => "👤",
                    SearchBy::Other => "🗂",
                };
                let names: &[String] = match self.search_by {
                    SearchBy::Folder => &self.unique_folders,
                    SearchBy::User => &self.unique_identities,
                    SearchBy::Other => &self.unique_others,
                };
                let mut clicked_idx: Option<usize> = None;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for &idx in &self.sidebar_cache {
                            let item = &names[idx];
                            let selected = match &self.selection {
                                Selection::Folder { name, .. } => {
                                    self.search_by == SearchBy::Folder && name == item
                                }
                                Selection::User { name, .. } => {
                                    self.search_by == SearchBy::User && name == item
                                }
                                Selection::Other { name, .. } => {
                                    self.search_by == SearchBy::Other && name == item
                                }
                                Selection::None => false,
                            };
                            if ui
                                .selectable_label(selected, format!("{icon} {item}"))
                                .clicked()
                            {
                                clicked_idx = Some(idx);
                            }
                        }
                    });
                if let Some(idx) = clicked_idx {
                    self.select_index(idx);
                }
            });
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            if self.records.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a pipe-delimited CSV (Folder|Identity|Rights|AccessControl|Inherited) to begin.");
                });
                return;
            }

            let mut clear_selection = false;
            let mut toggle_id: Option<String> = None;

            match &self.selection {
                Selection::None => {
                    draw_table(
                        ui,
                        &self.records,
                        &mut self.filtered_table,
                        &mut self.table_sort_col,
                        &mut self.table_sort_desc,
                    );
                }
                Selection::Folder { name, flat, .. } => {
                    draw_tree_view(ui, &self.records, "📁", name, flat, &mut clear_selection, &mut toggle_id);
                }
                Selection::User { name, flat, .. } => {
                    draw_tree_view(ui, &self.records, "👤", name, flat, &mut clear_selection, &mut toggle_id);
                }
                Selection::Other { name, flat, .. } => {
                    draw_tree_view(ui, &self.records, "🗂", name, flat, &mut clear_selection, &mut toggle_id);
                }
            }

            if clear_selection {
                self.selection = Selection::None;
            } else if let Some(id) = toggle_id {
                match &mut self.selection {
                    Selection::Folder { tree, expansion, flat, .. }
                    | Selection::User { tree, expansion, flat, .. }
                    | Selection::Other { tree, expansion, flat, .. } => {
                        expansion.toggle(id);
                        *flat = flatten_tree(tree, expansion);
                    }
                    Selection::None => {}
                }
            }
        });
    }
}

fn draw_table(
    ui: &mut egui::Ui,
    records: &[Record],
    filtered: &mut Vec<usize>,
    sort_col: &mut usize,
    sort_desc: &mut bool,
) {
    ui.label(
        egui::RichText::new(
            "All records — pick a folder or user on the left for a permissions tree.",
        )
        .weak(),
    );
    ui.add_space(4.0);
    let mut sort_clicked = None;

    TableBuilder::new(ui)
        .id_salt("full_table")
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .columns(Column::auto().resizable(true).at_least(80.0), COLUMNS.len())
        .header(26.0, |mut header| {
            for (i, name) in COLUMNS.iter().enumerate() {
                header.col(|ui| {
                    let label = if i == *sort_col {
                        format!("{name} {}", if *sort_desc { "▼" } else { "▲" })
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
            body.rows(22.0, filtered.len(), |mut row| {
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
        if *sort_col == col {
            *sort_desc = !*sort_desc;
        } else {
            *sort_col = col;
            *sort_desc = false;
        }
        let (sc, sd) = (*sort_col, *sort_desc);
        // sort_unstable_by: we don't need stability and it's noticeably
        // faster on large (million-row) inputs.
        filtered.sort_unstable_by(|&a, &b| {
            let ord = records[a].field(sc).cmp(records[b].field(sc));
            if sd { ord.reverse() } else { ord }
        });
    }
}

/// Renders a permissions tree from its precomputed flat row list. Only rows
/// actually visible in the scroll viewport are turned into widgets each
/// frame (`ScrollArea::show_rows`), which is what keeps this smooth even
/// when a user/folder has thousands of matching rows — a naive recursive
/// `CollapsingHeader` tree has to re-lay-out every open node every single
/// frame, which is what caused the lag.
fn draw_tree_view(
    ui: &mut egui::Ui,
    records: &[Record],
    icon: &str,
    name: &str,
    flat: &[FlatRow],
    clear_selection: &mut bool,
    toggle_id: &mut Option<String>,
) {
    ui.horizontal(|ui| {
        ui.heading(format!("{icon} {name}"));
        if ui.button("✕ Clear").clicked() {
            *clear_selection = true;
        }
    });
    ui.label(egui::RichText::new(format!("{} rows", flat.len())).weak());
    ui.add_space(4.0);

    const ROW_HEIGHT: f32 = 22.0;
    const INDENT: f32 = 18.0;

    egui::ScrollArea::vertical()
        .id_salt(format!("tree-{icon}-{name}"))
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, flat.len(), |ui, range| {
            for i in range {
                match &flat[i] {
                    FlatRow::Folder {
                        id,
                        name,
                        depth,
                        expanded,
                        has_children,
                    } => {
                        ui.horizontal(|ui| {
                            ui.add_space(*depth as f32 * INDENT);
                            let arrow = if !*has_children {
                                "  "
                            } else if *expanded {
                                "▼"
                            } else {
                                "▶"
                            };
                            if ui.button(format!("{arrow} 📂 {name}")).clicked() {
                                *toggle_id = Some(id.clone());
                            }
                        });
                    }
                    FlatRow::Entry { record_idx, depth } => {
                        let r = &records[*record_idx];
                        ui.horizontal(|ui| {
                            ui.add_space(*depth as f32 * INDENT + INDENT);
                            ui.label("🔑");
                            ui.label(egui::RichText::new(r.identity.as_ref()).strong());
                            ui.label(
                                egui::RichText::new(r.rights.as_ref())
                                    .color(egui::Color32::from_rgb(140, 200, 255)),
                            );
                            ui.label(egui::RichText::new(r.access_control.as_ref()).weak());
                            ui.label(egui::RichText::new(r.inherited.as_ref()).italics());
                        });
                    }
                }
            }
        });
}
