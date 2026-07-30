use crate::bridge::{
    CatalogSummary, ChatEvent, EndpointDetails, HardwareProfile, InstallOptions,
    InstallationValidationReport, MachineUsageSnapshot, PerformanceProfileReport,
    PerformanceSnapshot, PersistedModelEntry, PreflightReport, Recommendation,
    RemoteCredentialStatus, RuntimeDiagnostics, RuntimeMigrationOption, RuntimeMigrationReport,
    RuntimeStatus, SharedCoreAdapter,
};
use crate::managed_vllm::ManagedVllmSupport;
use lumen_source_runtime::ChatMessage;
use serde::Deserialize;
use tauri::{ipc::Channel, AppHandle, State};
use zeroize::Zeroizing;

use crate::remote::{RemoteConnectionReport, RemoteTargetConfig, RemoteTargetProfile};
use crate::settings::{
    validate_settings as validate_application_settings, ApplicationSettings, ExternalVllmConfig,
    ModelSettings, ModelSettingsSaveReport, OllamaConnectionReport, RuntimeSecretKind,
    SettingsSaveReport, SettingsValidationError, VllmConnectionReport,
};
use crate::storage::{CleanupReport, StorageReport};

#[tauri::command]
pub async fn telemetry_preference(
    core: State<'_, SharedCoreAdapter>,
) -> Result<Option<bool>, String> {
    core.telemetry_preference().await
}

#[tauri::command]
pub async fn set_telemetry_enabled(
    core: State<'_, SharedCoreAdapter>,
    enabled: bool,
) -> Result<(), String> {
    core.set_telemetry_enabled(enabled).await
}

#[tauri::command]
pub async fn load_settings(
    core: State<'_, SharedCoreAdapter>,
) -> Result<ApplicationSettings, String> {
    core.settings().await
}

#[tauri::command]
pub fn validate_settings(settings: ApplicationSettings) -> Vec<SettingsValidationError> {
    validate_application_settings(&settings)
}

#[tauri::command]
pub async fn save_settings(
    core: State<'_, SharedCoreAdapter>,
    settings: ApplicationSettings,
    confirm_network_exposure: bool,
) -> Result<SettingsSaveReport, String> {
    core.save_settings(settings, confirm_network_exposure).await
}

#[tauri::command]
pub async fn reset_settings(
    core: State<'_, SharedCoreAdapter>,
) -> Result<SettingsSaveReport, String> {
    core.reset_settings().await
}

#[tauri::command]
pub async fn storage_report(core: State<'_, SharedCoreAdapter>) -> Result<StorageReport, String> {
    core.storage_report().await
}

#[tauri::command]
pub async fn cleanup_storage(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
    confirmed: bool,
) -> Result<CleanupReport, String> {
    core.cleanup_storage(&entry_id, confirmed).await
}

#[tauri::command]
pub async fn export_connection_profiles(
    core: State<'_, SharedCoreAdapter>,
) -> Result<String, String> {
    core.export_connection_profiles().await
}

#[tauri::command]
pub async fn import_connection_profiles(
    core: State<'_, SharedCoreAdapter>,
    document: String,
) -> Result<Vec<PersistedModelEntry>, String> {
    core.import_connection_profiles(&document).await
}

#[tauri::command]
pub async fn inventory_action(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
    action: String,
    variant_id: Option<String>,
) -> Result<Vec<PersistedModelEntry>, String> {
    core.inventory_action(&entry_id, &action, variant_id.as_deref())
        .await
}

#[tauri::command]
pub async fn interrupted_install(
    core: State<'_, SharedCoreAdapter>,
) -> Result<Option<crate::bridge::InterruptedInstall>, String> {
    Ok(core.interrupted_install().await)
}

#[tauri::command]
pub async fn resume_interrupted_install(
    app: AppHandle,
    core: State<'_, SharedCoreAdapter>,
) -> Result<(), String> {
    core.resume_interrupted_install(app).await
}

#[tauri::command]
pub async fn discard_interrupted_install(
    core: State<'_, SharedCoreAdapter>,
    confirmed: bool,
) -> Result<(), String> {
    core.discard_interrupted_install(confirmed).await
}

#[tauri::command]
pub async fn test_ollama_connection(
    core: State<'_, SharedCoreAdapter>,
    settings: ApplicationSettings,
) -> Result<OllamaConnectionReport, String> {
    Ok(core.test_ollama_connection(settings).await)
}

#[tauri::command]
pub async fn restart_managed_ollama(
    core: State<'_, SharedCoreAdapter>,
) -> Result<OllamaConnectionReport, String> {
    core.restart_managed_ollama().await
}

#[tauri::command]
pub async fn runtime_secret_status(kind: RuntimeSecretKind) -> Result<bool, String> {
    crate::credential_store::runtime_secret_is_saved(kind).await
}

#[tauri::command]
pub async fn save_runtime_secret(kind: RuntimeSecretKind, secret: String) -> Result<(), String> {
    crate::credential_store::save_runtime_secret(kind, Zeroizing::new(secret)).await
}

#[tauri::command]
pub async fn delete_runtime_secret(kind: RuntimeSecretKind) -> Result<(), String> {
    crate::credential_store::delete_runtime_secret(kind).await
}

#[tauri::command]
pub async fn test_vllm_connection(
    core: State<'_, SharedCoreAdapter>,
    config: ExternalVllmConfig,
    api_key: Option<String>,
    entry_id: Option<String>,
) -> Result<VllmConnectionReport, String> {
    Ok(core
        .test_vllm_connection(
            config,
            api_key.filter(|key| !key.is_empty()).map(Zeroizing::new),
            entry_id.as_deref(),
        )
        .await)
}

#[tauri::command]
pub async fn save_vllm_model(
    core: State<'_, SharedCoreAdapter>,
    entry_id: Option<String>,
    display_name: String,
    config: ExternalVllmConfig,
    api_key: Option<String>,
    clear_api_key: bool,
) -> Result<PersistedModelEntry, String> {
    core.save_vllm_model(
        entry_id,
        display_name,
        config,
        api_key.filter(|key| !key.is_empty()).map(Zeroizing::new),
        clear_api_key,
    )
    .await
}

#[tauri::command]
pub async fn vllm_credential_status(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
) -> Result<bool, String> {
    core.vllm_credential_status(&entry_id).await
}

#[tauri::command]
pub async fn save_model_settings(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
    settings: ModelSettings,
    apply_restart: bool,
) -> Result<ModelSettingsSaveReport, String> {
    core.save_model_settings(&entry_id, settings, apply_restart)
        .await
}

#[tauri::command]
pub async fn model_settings_memory_warning(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
    settings: ModelSettings,
) -> Result<Option<String>, String> {
    core.model_settings_memory_warning(&entry_id, &settings)
        .await
}

#[tauri::command]
pub async fn managed_vllm_support(
    core: State<'_, SharedCoreAdapter>,
) -> Result<ManagedVllmSupport, String> {
    Ok(core.managed_vllm_support().await)
}

#[tauri::command]
pub async fn runtime_migration_options(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
) -> Result<Vec<RuntimeMigrationOption>, String> {
    core.runtime_migration_options(&entry_id).await
}

#[tauri::command]
pub async fn reinstall_with_runtime(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
    target_runtime: String,
) -> Result<RuntimeMigrationReport, String> {
    core.reinstall_with_runtime(&entry_id, &target_runtime)
        .await
}

#[tauri::command]
pub async fn runtime_diagnostics(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
) -> Result<RuntimeDiagnostics, String> {
    core.runtime_diagnostics(&entry_id).await
}

#[tauri::command]
pub async fn delete_managed_vllm_caches(
    core: State<'_, SharedCoreAdapter>,
    confirmed: bool,
) -> Result<(), String> {
    core.delete_managed_vllm_caches(confirmed).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    model_id: String,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    performance_profile: crate::settings::PerformanceProfile,
    license_basis: String,
    #[serde(default)]
    license_reference: Option<String>,
    license_acknowledged: bool,
    #[serde(default)]
    install_runtime: bool,
}

fn normalize_target_id(value: Option<String>) -> String {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "local".to_owned())
}

#[tauri::command]
pub async fn load_remote_targets(
    core: State<'_, SharedCoreAdapter>,
) -> Result<Vec<RemoteTargetProfile>, String> {
    Ok(core.remote_targets().await)
}

#[tauri::command]
pub async fn save_remote_target(
    core: State<'_, SharedCoreAdapter>,
    config: RemoteTargetConfig,
) -> Result<RemoteTargetProfile, String> {
    core.save_remote_target(config).await
}

#[tauri::command]
pub async fn check_remote_target(
    core: State<'_, SharedCoreAdapter>,
    config: RemoteTargetConfig,
    password: Option<String>,
) -> Result<RemoteConnectionReport, String> {
    core.check_remote_target(config, password.map(Zeroizing::new))
        .await
}

#[tauri::command]
pub async fn remote_credential_status(
    core: State<'_, SharedCoreAdapter>,
    target_id: String,
) -> Result<RemoteCredentialStatus, String> {
    core.remote_credential_status(&target_id).await
}

#[tauri::command]
pub async fn save_remote_password(
    core: State<'_, SharedCoreAdapter>,
    target_id: String,
    password: String,
) -> Result<(), String> {
    core.save_remote_password(&target_id, Zeroizing::new(password))
        .await
}

#[tauri::command]
pub async fn delete_remote_password(
    core: State<'_, SharedCoreAdapter>,
    target_id: String,
) -> Result<(), String> {
    core.delete_remote_password(&target_id).await
}

#[tauri::command]
pub async fn detect_hardware(
    core: State<'_, SharedCoreAdapter>,
    target_id: Option<String>,
    password: Option<String>,
) -> Result<HardwareProfile, String> {
    core.detect_hardware(
        &normalize_target_id(target_id),
        password.map(Zeroizing::new),
    )
    .await
}

#[tauri::command]
pub async fn machine_usage(
    core: State<'_, SharedCoreAdapter>,
    target_id: Option<String>,
    password: Option<String>,
) -> Result<MachineUsageSnapshot, String> {
    core.machine_usage(
        &normalize_target_id(target_id),
        password.map(Zeroizing::new),
    )
    .await
}

#[tauri::command]
pub async fn load_catalog(core: State<'_, SharedCoreAdapter>) -> Result<CatalogSummary, String> {
    core.load_catalog(false).await
}

#[tauri::command]
pub async fn refresh_catalog(core: State<'_, SharedCoreAdapter>) -> Result<CatalogSummary, String> {
    core.load_catalog(true).await
}

#[tauri::command]
pub async fn get_recommendations(
    core: State<'_, SharedCoreAdapter>,
    intent: String,
    target_id: Option<String>,
) -> Result<Vec<Recommendation>, String> {
    core.recommendations(&intent, &normalize_target_id(target_id))
        .await
}

#[tauri::command]
pub async fn run_preflight(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
    target_id: Option<String>,
) -> Result<PreflightReport, String> {
    core.preflight(&model_id, &normalize_target_id(target_id))
        .await
}

#[tauri::command]
pub async fn performance_profile(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
    target_id: Option<String>,
    profile: crate::settings::PerformanceProfile,
) -> Result<PerformanceProfileReport, String> {
    core.performance_profile(&model_id, &normalize_target_id(target_id), profile)
        .await
}

#[tauri::command]
pub async fn install_model(
    app: AppHandle,
    core: State<'_, SharedCoreAdapter>,
    request: InstallRequest,
) -> Result<(), String> {
    core.install(
        app,
        request.model_id,
        normalize_target_id(request.target_id),
        InstallOptions {
            performance_profile: request.performance_profile,
            license_basis: request.license_basis,
            license_reference: request.license_reference,
            license_acknowledged: request.license_acknowledged,
            install_runtime: request.install_runtime,
        },
    )
    .await
}

#[tauri::command]
pub async fn validate_installed_model(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
    target_id: Option<String>,
    profile: crate::settings::PerformanceProfile,
    leave_running: bool,
) -> Result<InstallationValidationReport, String> {
    core.validate_installation(
        &model_id,
        &normalize_target_id(target_id),
        profile,
        leave_running,
    )
    .await
}

#[tauri::command]
pub async fn remove_incomplete_install(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
    target_id: Option<String>,
    confirmed: bool,
) -> Result<(), String> {
    core.remove_incomplete_install(&model_id, &normalize_target_id(target_id), confirmed)
        .await
}

#[tauri::command]
pub async fn cancel_install(core: State<'_, SharedCoreAdapter>) -> Result<bool, String> {
    Ok(core.cancel_install().await)
}

#[tauri::command]
pub async fn start_runtime(
    core: State<'_, SharedCoreAdapter>,
    entry_id: Option<String>,
    model_id: String,
    target_id: Option<String>,
    password: Option<String>,
) -> Result<RuntimeStatus, String> {
    core.start(
        entry_id.as_deref(),
        model_id,
        normalize_target_id(target_id),
        password.map(Zeroizing::new),
    )
    .await
}

#[tauri::command]
pub async fn stop_runtime(
    core: State<'_, SharedCoreAdapter>,
    entry_id: Option<String>,
    model_id: String,
    target_id: Option<String>,
    password: Option<String>,
) -> Result<RuntimeStatus, String> {
    core.stop(
        entry_id.as_deref(),
        model_id,
        normalize_target_id(target_id),
        password.map(Zeroizing::new),
    )
    .await
}

#[tauri::command]
pub async fn runtime_status(core: State<'_, SharedCoreAdapter>) -> Result<RuntimeStatus, String> {
    core.status().await
}

#[tauri::command]
pub async fn model_performance(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
    model_id: String,
    runtime_model_id: String,
    target_id: Option<String>,
) -> Result<PerformanceSnapshot, String> {
    core.performance(
        &entry_id,
        &model_id,
        &runtime_model_id,
        &normalize_target_id(target_id),
    )
    .await
}

#[tauri::command]
pub async fn endpoint_details(
    core: State<'_, SharedCoreAdapter>,
    target_id: Option<String>,
) -> Result<EndpointDetails, String> {
    core.endpoint(&normalize_target_id(target_id)).await
}

#[tauri::command]
pub async fn model_endpoint_details(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
    model_id: String,
    runtime_model_id: String,
    target_id: Option<String>,
) -> Result<EndpointDetails, String> {
    core.model_endpoint(
        &entry_id,
        &model_id,
        &runtime_model_id,
        &normalize_target_id(target_id),
    )
    .await
}

#[tauri::command]
pub async fn chat_with_model(
    core: State<'_, SharedCoreAdapter>,
    entry_id: String,
    model_id: String,
    runtime_model_id: String,
    target_id: Option<String>,
    messages: Vec<ChatMessage>,
    on_event: Channel<ChatEvent>,
) -> Result<(), String> {
    let reporter = |event| {
        let _ = on_event.send(event);
    };
    core.chat(
        &entry_id,
        &model_id,
        &runtime_model_id,
        &normalize_target_id(target_id),
        messages,
        &reporter,
    )
    .await
}

#[tauri::command]
pub async fn cancel_chat(core: State<'_, SharedCoreAdapter>) -> Result<bool, String> {
    Ok(core.cancel_chat().await)
}

#[tauri::command]
pub async fn load_models(
    core: State<'_, SharedCoreAdapter>,
) -> Result<Vec<PersistedModelEntry>, String> {
    core.load_models().await
}

#[tauri::command]
pub async fn save_models(
    core: State<'_, SharedCoreAdapter>,
    models: Vec<PersistedModelEntry>,
) -> Result<(), String> {
    core.save_models(models).await
}

#[tauri::command]
pub async fn remove_model(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
) -> Result<Vec<PersistedModelEntry>, String> {
    core.remove_model(&model_id).await
}
