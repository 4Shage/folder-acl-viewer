mod app;
mod loader;
mod model;
mod style;

use app::FolderAclApp;

fn main() -> eframe::Result<()> {
    let cli_path = std::env::args().nth(1);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let handle = runtime.handle().clone();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1150.0, 680.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Folder ACL Viewer",
        options,
        Box::new(move |cc| {
            style::apply_style(&cc.egui_ctx);
            let mut app = FolderAclApp::new(handle);
            if let Some(path) = cli_path {
                app.request_load(std::path::PathBuf::from(path));
            }
            Ok(Box::new(app))
        }),
    );

    drop(runtime);
    result
}
