pub mod bridge;
mod bridge_types;
pub mod commands;
mod credential_store;
mod model_reconciliation;
pub mod remote;
pub mod telemetry;

use bridge::SharedCoreAdapter;
use tauri::Manager;

pub fn run() {
    if let Some(exit_code) = remote::run_askpass_helper_if_requested() {
        std::process::exit(exit_code);
    }
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
            app.state::<SharedCoreAdapter>().retry_telemetry_upload();
            if let (Some(window), Some(icon)) =
                (app.get_webview_window("main"), app.default_window_icon())
            {
                window.set_icon(icon.clone())?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::telemetry_preference,
            commands::set_telemetry_enabled,
            commands::detect_hardware,
            commands::machine_usage,
            commands::load_remote_targets,
            commands::save_remote_target,
            commands::check_remote_target,
            commands::remote_credential_status,
            commands::save_remote_password,
            commands::delete_remote_password,
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
