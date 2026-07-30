pub mod bridge;
mod bridge_types;
pub mod commands;
mod credential_store;
mod managed_vllm;
mod model_reconciliation;
pub mod remote;
mod runtime_registry;
pub mod settings;
mod sharing;
mod storage;
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
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = app_handle
                    .state::<SharedCoreAdapter>()
                    .start_configured_sharing()
                    .await
                {
                    eprintln!("could not start configured sharing gateway: {error}");
                }
            });
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
            commands::load_settings,
            commands::validate_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::sharing_status,
            commands::generate_sharing_token,
            commands::revoke_sharing_token,
            commands::configure_sharing,
            commands::diagnostic_bundle,
            commands::export_state_backup,
            commands::restore_state_backup,
            commands::safe_reset,
            commands::storage_report,
            commands::cleanup_storage,
            commands::export_connection_profiles,
            commands::import_connection_profiles,
            commands::inventory_action,
            commands::interrupted_install,
            commands::resume_interrupted_install,
            commands::discard_interrupted_install,
            commands::test_ollama_connection,
            commands::restart_managed_ollama,
            commands::runtime_secret_status,
            commands::save_runtime_secret,
            commands::delete_runtime_secret,
            commands::test_vllm_connection,
            commands::save_vllm_model,
            commands::vllm_credential_status,
            commands::save_model_settings,
            commands::model_settings_memory_warning,
            commands::model_update_plan,
            commands::apply_model_update,
            commands::rollback_model_update,
            commands::resource_start_plan,
            commands::queued_operations,
            commands::dismiss_queued_operation,
            commands::managed_vllm_support,
            commands::runtime_migration_options,
            commands::reinstall_with_runtime,
            commands::runtime_diagnostics,
            commands::delete_managed_vllm_caches,
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
            commands::performance_profile,
            commands::install_model,
            commands::validate_installed_model,
            commands::remove_incomplete_install,
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
