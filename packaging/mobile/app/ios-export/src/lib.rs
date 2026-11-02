//! Native-only bridge to the iOS document export picker.
#![cfg(target_os = "ios")]

use std::path::Path;
use tauri::{
    AppHandle, Manager, Runtime,
    plugin::{Builder, PluginHandle, TauriPlugin},
};

tauri::ios_plugin_binding!(init_plugin_pdf_export);

struct Exporter<R: Runtime>(PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("pdf-export")
        .setup(|app, api| {
            app.manage(Exporter(api.register_ios_plugin(init_plugin_pdf_export)?));
            Ok(())
        })
        .build()
}

#[derive(serde::Serialize)]
struct Request<'a> {
    path: &'a Path,
}

#[derive(serde::Deserialize)]
struct ResultPayload {
    exported: bool,
}

/// Wait for Files to finish copying the staged file. Call from a worker, never the UI thread.
pub fn export<R: Runtime>(app: &AppHandle<R>, path: &Path) -> Result<bool, String> {
    app.state::<Exporter<R>>().0.run_mobile_plugin::<ResultPayload>("exportPdf", Request { path }).map(|v| v.exported).map_err(|e| e.to_string())
}
