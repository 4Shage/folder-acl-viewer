use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use tokio::runtime::Handle;

use crate::loader;
use crate::model::{
    COLUMNS, Record, TreeNode, build_other_tree, build_path_tree, unique_folders, unique_identities,
};

#[derive(PartialEq, Clone, Copy)]
enum SearchBy {
    Folder,
    User,
}

enum Selection {
    None,
    Folder(String),
    User(String),
}

pub struct FolderAclApp {
    records: Vec<Record>,
    filtered_table: Vec<usize>,

    search_by: SearchBy,
    sidebar_query: String,
    selection: Selection,

    table_sort_col: usize,
    table_sort_desc: bool,

    loaded_path: Option<String>,
    error: Option<String>,
    loading: bool,

    rt: Handle,
    result_tx: Sender<Result<(PathBuf, Vec<Record>), String>>,
    result_rx: Receiver<Result<(PathBuf, Vec<Record>), String>>,
}

impl FolderAclApp {
    pub fn new(rt: Handle) -> Self {
        let (result_tx, result_rx) = channel();
        Self {
            records: Vec::new(),
            filtered_table: Vec::new(),
            search_by: SearchBy::Folder,
            sidebar_query: String::new(),
            selection: Selection::None,
            table_sort_col: 0,
            table_sort_desc: false,
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
            let _ = tx.send(res.map(|records| (path, records)));
        });
    }

    fn on_loaded(&mut self, path: PathBuf, records: Vec<Record>) {
        self.records = records;
        self.loaded_path = Some(path.display().to_string());
        self.selection = Selection::None;
        self.apply_table_sort();
    }

    fn apply_table_sort(&mut self) {
        self.filtered_table = (0..self.records.len()).collect();
        let col = self.table_sort_col;
        let desc = self.table_sort_desc;
        let records = &self.records;
        self.filtered_table.sort_by(|&a, &b| {
            let ord = records[a].field(col).cmp(records[b].field(col));
            if desc { ord.reverse() } else { ord }
        });
    }

    fn set_table_sort(&mut self, col: usize) {
        if self.table_sort_col == col {
            self.table_sort_desc = !self.table_sort_desc;
        } else {
            self.table_sort_col = col;
            self.table_sort_desc = false;
        }
        self.apply_table_sort();
    }

    fn sidebar_items(&self) -> Vec<String> {
        let all = match self.search_by {
            SearchBy::Folder => unique_folders(&self.records),
            SearchBy::User => unique_identities(&self.records),
        };
        if self.sidebar_query.is_empty() {
            all
        } else {
            let q = self.sidebar_query.to_lowercase();
            all.into_iter()
                .filter(|v| v.to_lowercase().contains(&q))
                .collect()
        }
    }
}

impl eframe::App for FolderAclApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(result) = self.result_rx.try_recv() {
            self.loading = false;
            match result {
                Ok((path, records)) => self.on_loaded(path, records),
                Err(e) => self.error = Some(e),
            }
        }
        if self.loading {
            ctx.request_repaint();
        }

        self.top_panel(ctx);
        self.sidebar_panel(ctx);
        self.central_panel(ctx);
    }
}

impl FolderAclApp {
    fn top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
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

    fn sidebar_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(270.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(self.search_by == SearchBy::Folder, "📁 Folders")
                        .clicked()
                        && self.search_by != SearchBy::Folder
                    {
                        self.search_by = SearchBy::Folder;
                        self.sidebar_query.clear();
                    }
                    if ui
                        .selectable_label(self.search_by == SearchBy::User, "👤 Users")
                        .clicked()
                        && self.search_by != SearchBy::User
                    {
                        self.search_by = SearchBy::User;
                        self.sidebar_query.clear();
                    }
                });
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.sidebar_query)
                        .hint_text(match self.search_by {
                            SearchBy::Folder => "🔍 Search folders...",
                            SearchBy::User => "🔍 Search users...",
                        })
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.separator();

                let items = self.sidebar_items();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for item in items {
                            let selected = match &self.selection {
                                Selection::Folder(f) => {
                                    self.search_by == SearchBy::Folder && f == &item
                                }
                                Selection::User(u) => {
                                    self.search_by == SearchBy::User && u == &item
                                }
                                Selection::None => false,
                            };
                            let icon = match self.search_by {
                                SearchBy::Folder => "📁",
                                SearchBy::User => "👤",
                            };
                            if ui
                                .selectable_label(selected, format!("{icon} {item}"))
                                .clicked()
                            {
                                self.selection = match self.search_by {
                                    SearchBy::Folder => Selection::Folder(item.clone()),
                                    SearchBy::User => Selection::User(item.clone()),
                                };
                            }
                        }
                    });
            });
    }

    fn central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.records.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a pipe-delimited CSV (Folder|Identity|Rights|AccessControl|Inherited) to begin.");
                });
                return;
            }

            match &self.selection {
                Selection::None => self.draw_table(ui),
                Selection::Folder(folder) => {
                    let folder = folder.clone();
                    self.draw_folder_tree(ui, &folder);
                }
                Selection::User(user) => {
                    let user = user.clone();
                    self.draw_user_tree(ui, &user);
                }
            }
        });
    }

    fn draw_table(&mut self, ui: &mut egui::Ui) {
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
                        let label = if i == self.table_sort_col {
                            format!("{name} {}", if self.table_sort_desc { "▼" } else { "▲" })
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
                let filtered = &self.filtered_table;
                let records = &self.records;
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
            self.set_table_sort(col);
        }
    }

    fn draw_folder_tree(&mut self, ui: &mut egui::Ui, folder: &str) {
        ui.horizontal(|ui| {
            ui.heading(format!("📁 {folder}"));
            if ui.button("✕ Clear").clicked() {
                self.selection = Selection::None;
            }
        });
        ui.add_space(4.0);

        let indices: Vec<usize> = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.folder == folder)
            .map(|(i, _)| i)
            .collect();
        let tree = build_other_tree(&self.records, &indices);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                render_entries(ui, &self.records, &tree.entries, false);
                for (name, child) in &tree.children {
                    render_node(
                        ui,
                        &self.records,
                        name,
                        child,
                        false,
                        &format!("folder-{folder}"),
                    );
                }
            });
    }

    fn draw_user_tree(&mut self, ui: &mut egui::Ui, user: &str) {
        ui.horizontal(|ui| {
            ui.heading(format!("👤 {user}"));
            if ui.button("✕ Clear").clicked() {
                self.selection = Selection::None;
            }
        });
        ui.add_space(4.0);

        let indices: Vec<usize> = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.identity == user)
            .map(|(i, _)| i)
            .collect();
        let tree = build_path_tree(&self.records, &indices);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                render_entries(ui, &self.records, &tree.entries, true);
                for (name, child) in &tree.children {
                    render_node(
                        ui,
                        &self.records,
                        name,
                        child,
                        true,
                        &format!("user-{user}"),
                    );
                }
            });
    }
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
