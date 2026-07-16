use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use tokio::runtime::Handle;

use crate::loader::{self, LoadedData};
use crate::model::{COLUMNS, Record, TreeNode, build_other_tree, build_path_tree};

#[derive(PartialEq, Clone, Copy)]
enum SearchBy {
    Folder,
    User,
}

enum Selection {
    None,
    Folder { name: String, tree: TreeNode },
    User { name: String, tree: TreeNode },
}

pub struct FolderAclApp {
    records: Vec<Record>,
    folder_index: HashMap<String, Vec<usize>>,
    identity_index: HashMap<String, Vec<usize>>,
    unique_folders: Vec<String>,
    unique_identities: Vec<String>,

    filtered_table: Vec<usize>,
    table_sort_col: usize,
    table_sort_desc: bool,

    search_by: SearchBy,
    sidebar_query: String,
    sidebar_cache: Vec<String>,
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
            unique_folders: Vec::new(),
            unique_identities: Vec::new(),
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
        self.unique_folders = data.unique_folders;
        self.unique_identities = data.unique_identities;

        self.loaded_path = Some(path.display().to_string());
        self.selection = Selection::None;
        self.table_sort_col = 0;
        self.table_sort_desc = false;
        self.filtered_table = (0..self.records.len()).collect();
        self.refresh_sidebar_cache();
    }

    fn refresh_sidebar_cache(&mut self) {
        let source = match self.search_by {
            SearchBy::Folder => &self.unique_folders,
            SearchBy::User => &self.unique_identities,
        };
        self.sidebar_cache = if self.sidebar_query.is_empty() {
            source.clone()
        } else {
            let q = self.sidebar_query.to_lowercase();
            source
                .iter()
                .filter(|v| v.to_lowercase().contains(&q))
                .cloned()
                .collect()
        };
    }

    /// Looks up the pre-built index (O(1)) instead of scanning all records,
    /// so selecting a folder/user stays instant even at millions of rows.
    fn select(&mut self, name: String) {
        match self.search_by {
            SearchBy::Folder => {
                let indices = self.folder_index.get(&name).cloned().unwrap_or_default();
                let tree = build_other_tree(&self.records, &indices);
                self.selection = Selection::Folder { name, tree };
            }
            SearchBy::User => {
                let indices = self.identity_index.get(&name).cloned().unwrap_or_default();
                let tree = build_path_tree(&self.records, &indices);
                self.selection = Selection::User { name, tree };
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
                });
                ui.add_space(4.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.sidebar_query)
                        .hint_text(match self.search_by {
                            SearchBy::Folder => "🔍 Search folders...",
                            SearchBy::User => "🔍 Search users...",
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
                };
                let mut clicked_item: Option<String> = None;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for item in &self.sidebar_cache {
                            let selected = match &self.selection {
                                Selection::Folder { name, .. } => {
                                    self.search_by == SearchBy::Folder && name == item
                                }
                                Selection::User { name, .. } => {
                                    self.search_by == SearchBy::User && name == item
                                }
                                Selection::None => false,
                            };
                            if ui
                                .selectable_label(selected, format!("{icon} {item}"))
                                .clicked()
                            {
                                clicked_item = Some(item.clone());
                            }
                        }
                    });
                if let Some(item) = clicked_item {
                    self.select(item);
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
                Selection::Folder { name, tree } => {
                    draw_tree_view(ui, &self.records, "📁", name, tree, false, &mut clear_selection);
                }
                Selection::User { name, tree } => {
                    draw_tree_view(ui, &self.records, "👤", name, tree, true, &mut clear_selection);
                }
            }

            if clear_selection {
                self.selection = Selection::None;
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
        filtered.sort_by(|&a, &b| {
            let ord = records[a].field(sc).cmp(records[b].field(sc));
            if sd { ord.reverse() } else { ord }
        });
    }
}

fn draw_tree_view(
    ui: &mut egui::Ui,
    records: &[Record],
    icon: &str,
    name: &str,
    tree: &TreeNode,
    show_identity: bool,
    clear_selection: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.heading(format!("{icon} {name}"));
        if ui.button("✕ Clear").clicked() {
            *clear_selection = true;
        }
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .id_salt(format!("tree-{icon}-{name}"))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            render_entries(ui, records, &tree.entries, show_identity);
            for (child_name, child) in &tree.children {
                render_node(
                    ui,
                    records,
                    child_name,
                    child,
                    show_identity,
                    &format!("{icon}{name}"),
                );
            }
        });
}

fn render_node(
    ui: &mut egui::Ui,
    records: &[Record],
    name: &str,
    node: &TreeNode,
    show_identity: bool,
    id_prefix: &str,
) {
    let id = format!("{id_prefix}/{name}");
    egui::CollapsingHeader::new(format!("📂 {name}"))
        .id_salt(id.clone())
        .default_open(false)
        .show(ui, |ui| {
            render_entries(ui, records, &node.entries, show_identity);
            for (child_name, child) in &node.children {
                render_node(ui, records, child_name, child, show_identity, &id);
            }
        });
}

fn render_entries(ui: &mut egui::Ui, records: &[Record], entries: &[usize], show_identity: bool) {
    for &idx in entries {
        let r = &records[idx];
        ui.horizontal(|ui| {
            ui.label("🔑");
            if show_identity {
                ui.label(egui::RichText::new(&r.identity).strong());
            }
            ui.label(egui::RichText::new(&r.rights).color(egui::Color32::from_rgb(140, 200, 255)));
            ui.label(egui::RichText::new(&r.access_control).weak());
            ui.label(egui::RichText::new(&r.inherited).italics());
        });
    }
}
