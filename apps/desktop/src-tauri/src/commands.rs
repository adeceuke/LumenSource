use crate::bridge::{
    CatalogSummary, ChatEvent, EndpointDetails, HardwareProfile, PerformanceSnapshot,
    PersistedModelEntry, PreflightReport, Recommendation, RuntimeStatus, SharedCoreAdapter,
};
use lumen_source_runtime::ChatMessage;
use serde::Deserialize;
use tauri::{ipc::Channel, AppHandle, State};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    model_id: String,
    license_basis: String,
    #[serde(default)]
    license_reference: Option<String>,
    license_acknowledged: bool,
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
    core.install(
        app,
        request.model_id,
        request.license_basis,
        request.license_reference,
        request.license_acknowledged,
    )
    .await
}

#[tauri::command]
pub async fn cancel_install(core: State<'_, SharedCoreAdapter>) -> Result<bool, String> {
    Ok(core.cancel_install().await)
}

#[tauri::command]
pub async fn start_runtime(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
) -> Result<RuntimeStatus, String> {
    core.start(model_id).await
}

#[tauri::command]
pub async fn stop_runtime(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
) -> Result<RuntimeStatus, String> {
    core.stop(model_id).await
}

#[tauri::command]
pub async fn runtime_status(core: State<'_, SharedCoreAdapter>) -> Result<RuntimeStatus, String> {
    core.status().await
}

#[tauri::command]
pub async fn model_performance(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
    runtime_model_id: String,
) -> Result<PerformanceSnapshot, String> {
    core.performance(&model_id, &runtime_model_id).await
}

#[tauri::command]
pub async fn endpoint_details(
    core: State<'_, SharedCoreAdapter>,
) -> Result<EndpointDetails, String> {
    core.endpoint().await
}

#[tauri::command]
pub async fn model_endpoint_details(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
    runtime_model_id: String,
) -> Result<EndpointDetails, String> {
    core.model_endpoint(&model_id, &runtime_model_id).await
}

#[tauri::command]
pub async fn chat_with_model(
    core: State<'_, SharedCoreAdapter>,
    model_id: String,
    runtime_model_id: String,
    messages: Vec<ChatMessage>,
    on_event: Channel<ChatEvent>,
) -> Result<(), String> {
    let reporter = |event| {
        let _ = on_event.send(event);
    };
    core.chat(&model_id, &runtime_model_id, messages, &reporter)
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
