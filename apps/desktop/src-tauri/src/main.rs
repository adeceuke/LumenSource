#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use lumen_source_desktop::{bridge::SharedCoreAdapter, commands};

fn main() {
    let core = match SharedCoreAdapter::new() {
        Ok(core) => core,
        Err(error) => {
            eprintln!("failed to initialize Lumen Source: {error}");
            std::process::exit(1);
        }
    };
    tauri::Builder::default()
        .manage(core)
        .invoke_handler(tauri::generate_handler![
            commands::detect_hardware,
            commands::load_catalog,
            commands::refresh_catalog,
            commands::get_recommendations,
            commands::run_preflight,
            commands::install_model,
            commands::start_runtime,
            commands::stop_runtime,
            commands::runtime_status,
            commands::endpoint_details,
            commands::load_models,
            commands::save_models,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|error| {
            eprintln!("failed to run Lumen Source desktop application: {error}");
            std::process::exit(1);
        });
}
