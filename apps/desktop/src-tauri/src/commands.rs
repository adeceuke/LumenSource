use crate::bridge::{
    CatalogSummary, EndpointDetails, HardwareProfile, PersistedModelEntry, PreflightReport,
    Recommendation, RuntimeStatus, SharedCoreAdapter,
};
use serde::Deserialize;
use tauri::{AppHandle, State};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    model_id: String,
}

#[tauri::command]
pub async fn detect_hardware(
    core: State<'_, SharedCoreAdapter>,
) -> Result<HardwareProfile, String> {
    core.detect_hardware().await
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
) -> Result<Vec<Recommendation>, String> {
    core.recommendations(&intent).await
}

#[tauri::command]
pub async fn run_preflight(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
) -> Result<PreflightReport, String> {
    core.preflight(&model_id).await
}

#[tauri::command]
pub async fn install_model(
    app: AppHandle,
    core: State<'_, SharedCoreAdapter>,
    request: InstallRequest,
) -> Result<(), String> {
    core.install(app, request.model_id).await
}

#[tauri::command]
pub async fn start_runtime(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
) -> Result<RuntimeStatus, String> {
    core.start(model_id).await
}

#[tauri::command]
pub async fn stop_runtime(core: State<'_, SharedCoreAdapter>) -> Result<RuntimeStatus, String> {
    core.stop().await
}

#[tauri::command]
pub async fn runtime_status(core: State<'_, SharedCoreAdapter>) -> Result<RuntimeStatus, String> {
    core.status().await
}

#[tauri::command]
pub async fn endpoint_details(
    core: State<'_, SharedCoreAdapter>,
) -> Result<EndpointDetails, String> {
    core.endpoint().await
}

#[tauri::command]
pub async fn load_models(core: State<'_, SharedCoreAdapter>) -> Result<Vec<PersistedModelEntry>, String> {
    core.load_models().await
}

#[tauri::command]
pub async fn save_models(
    core: State<'_, SharedCoreAdapter>,
    models: Vec<PersistedModelEntry>,
) -> Result<(), String> {
    core.save_models(models).await
}
