#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use lumen_source_desktop::{bridge::SharedCoreAdapter, commands};
use tauri::Manager;

fn main() {
    let core = match SharedCoreAdapter::new() {
        Ok(core) => core,
        Err(error) => {
            eprintln!("failed to initialize Lumen Source: {error}");
            std::process::exit(1);
        }
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(core)
        .setup(|app| {
            if let (Some(window), Some(icon)) =
                (app.get_webview_window("main"), app.default_window_icon())
            {
                window.set_icon(icon.clone())?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::detect_hardware,
            commands::load_catalog,
            commands::refresh_catalog,
            commands::get_recommendations,
            commands::run_preflight,
            commands::install_model,
            commands::cancel_install,
            commands::start_runtime,
            commands::stop_runtime,
            commands::runtime_status,
            commands::model_performance,
            commands::endpoint_details,
            commands::model_endpoint_details,
            commands::chat_with_model,
            commands::cancel_chat,
            commands::load_models,
            commands::save_models,
            commands::remove_model,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|error| {
            eprintln!("failed to run Lumen Source desktop application: {error}");
            std::process::exit(1);
        });
}
