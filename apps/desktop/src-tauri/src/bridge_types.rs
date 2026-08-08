use std::time::{SystemTime, UNIX_EPOCH};

use lumen_source_catalog::{ExternalEvaluation, License};
use lumen_source_hardware::{AcceleratorKind, HardwareFacts, UsageSnapshot};
use serde::{Deserialize, Serialize};

use crate::runtime_registry::RuntimeCapabilities;
use crate::settings::{ModelSettings, PerformanceProfile};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedModelEntry {
    pub id: String,
    pub name: String,
    pub model_id: String,
    pub model_name: String,
    #[serde(default = "default_runtime_id")]
    pub runtime_id: String,
    #[serde(default)]
    pub runtime_model_id: Option<String>,
    #[serde(default)]
    pub runtime_capabilities: RuntimeCapabilities,
    #[serde(default)]
    pub model_settings: Option<ModelSettings>,
    #[serde(default)]
    pub installation_validation: Option<InstallationValidationReport>,
    #[serde(default)]
    pub performance_test: Option<PerformanceTestResult>,
    #[serde(default = "available_inventory_status")]
    pub inventory_status: String,
    #[serde(default)]
    pub discovered: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub last_seen_at: Option<String>,
    #[serde(default)]
    pub rollback: Option<ModelRevisionSnapshot>,
    pub version: String,
    pub location: String,
    #[serde(default = "local_target_id")]
    pub target_id: String,
    #[serde(default)]
    pub target_name: Option<String>,
    pub running: bool,
    #[serde(default = "default_managed")]
    pub managed: bool,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub license_basis: Option<String>,
    #[serde(default)]
    pub license_reference: Option<String>,
    #[serde(default)]
    pub license_acknowledged_at: Option<String>,
    #[serde(default)]
    pub license_profile_id: Option<String>,
    #[serde(default)]
    pub license_name: Option<String>,
    #[serde(default)]
    pub license_url: Option<String>,
    #[serde(default)]
    pub license_reviewed_at: Option<String>,
    #[serde(default)]
    pub license_catalog_version: Option<String>,
    pub logs: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceProfileReport {
    pub profile: PerformanceProfile,
    pub settings: ModelSettings,
    pub summary: String,
    pub accelerator: String,
    pub context_length: Option<u32>,
    pub concurrent_requests: u32,
    pub minimum_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub fits_detected_memory: bool,
    pub warnings: Vec<String>,
    pub hardware_summary: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationValidationReport {
    pub passed: bool,
    pub capability: String,
    pub runtime_id: String,
    pub runtime_model_id: String,
    pub message: String,
    pub accelerator: Option<String>,
    pub hardware_summary: Option<String>,
    pub effective_context_length: Option<u32>,
    pub validated_at: String,
    pub running: bool,
    pub settings: ModelSettings,
    pub checks: Vec<InstallationValidationCheck>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationValidationCheck {
    pub id: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedInstall {
    pub model_id: String,
    pub target_id: String,
    pub performance_profile: PerformanceProfile,
    pub license_basis: String,
    pub license_reference: Option<String>,
    pub license_acknowledged: bool,
    pub install_runtime: bool,
    pub started_at: String,
    pub recovery: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRevisionSnapshot {
    pub runtime_model_id: String,
    pub rollback_artifact_id: Option<String>,
    pub runtime_executable: Option<String>,
    pub digest: Option<String>,
    pub version: String,
    pub model_settings: ModelSettings,
    pub installation_validation: Option<InstallationValidationReport>,
    pub saved_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUpdatePlan {
    pub available: bool,
    pub classification: String,
    pub kinds: Vec<String>,
    pub current_revision: String,
    pub candidate_revision: String,
    pub download_bytes: u64,
    pub additional_disk_bytes: u64,
    pub license_changed: bool,
    pub requires_license_acknowledgement: bool,
    pub compatibility: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStartPlan {
    pub can_start: bool,
    pub required_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub required_vram_bytes: u64,
    pub available_vram_bytes: Option<u64>,
    pub consumers: Vec<ResourceConsumer>,
    pub stoppable_model_ids: Vec<String>,
    pub pinned_model_ids: Vec<String>,
    pub waiting_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceConsumer {
    pub model_id: String,
    pub name: String,
    pub memory_bytes: u64,
    pub vram_bytes: u64,
    pub pinned: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedOperation {
    pub model_id: String,
    pub action: String,
    pub queued_at: String,
    pub needs_retry: bool,
}

fn default_managed() -> bool {
    true
}

fn default_runtime_id() -> String {
    crate::runtime_registry::OLLAMA_RUNTIME.to_owned()
}

fn available_inventory_status() -> String {
    "available".to_owned()
}

pub(crate) fn local_target_id() -> String {
    "local".to_owned()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    pub cpu: String,
    pub cpu_cores: usize,
    pub cpu_frequency_mhz: Option<u64>,
    pub memory_bytes: u64,
    pub memory_kind: Option<String>,
    pub memory_speed_mts: Option<u64>,
    pub gpu: Option<GpuProfile>,
    pub platform: String,
    pub storage: StorageProfile,
}

impl From<&HardwareFacts> for HardwareProfile {
    fn from(facts: &HardwareFacts) -> Self {
        let gpu = facts.accelerators.first().map(|device| GpuProfile {
            name: device.name.clone(),
            memory_bytes: device.total_vram_bytes,
            backend: accelerator_backend(device.kind).to_owned(),
        });
        Self {
            cpu: facts
                .cpu
                .model
                .clone()
                .unwrap_or_else(|| facts.cpu.architecture.clone()),
            cpu_cores: facts.cpu.logical_cores,
            cpu_frequency_mhz: facts.cpu.frequency_mhz,
            memory_bytes: facts.total_ram_bytes,
            memory_kind: facts.memory.kind.clone(),
            memory_speed_mts: facts.memory.speed_mts,
            gpu,
            platform: format!("{} {}", facts.os.family, facts.os.architecture),
            storage: StorageProfile {
                mount_point: facts.storage.mount_point.display().to_string(),
                total_bytes: facts.storage.total_bytes,
                available_bytes: facts.storage.available_bytes,
            },
        }
    }
}

pub(crate) fn accelerator_backend(kind: AcceleratorKind) -> &'static str {
    match kind {
        AcceleratorKind::Nvidia => "cuda",
        AcceleratorKind::Amd => "rocm",
        AcceleratorKind::Intel => "intel",
        AcceleratorKind::Other => "other",
    }
}

pub(crate) fn deployment_kind(target_id: &str) -> String {
    if target_id == "local" {
        "local"
    } else {
        "remote"
    }
    .to_owned()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuProfile {
    pub name: String,
    pub memory_bytes: Option<u64>,
    pub backend: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProfile {
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineUsageSnapshot {
    pub target_id: String,
    pub sampled_at_unix_ms: u64,
    pub cpu_utilization_percent: f32,
    pub used_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub accelerators: Vec<MachineAcceleratorUsage>,
}

impl MachineUsageSnapshot {
    pub(crate) fn from_usage(target_id: &str, usage: UsageSnapshot) -> Self {
        Self {
            target_id: target_id.to_owned(),
            sampled_at_unix_ms: usage.sampled_at_unix_ms,
            cpu_utilization_percent: usage.cpu_utilization_percent,
            used_memory_bytes: usage.used_ram_bytes,
            available_memory_bytes: usage.available_ram_bytes,
            accelerators: usage
                .accelerators
                .into_iter()
                .map(|accelerator| MachineAcceleratorUsage {
                    name: accelerator.name,
                    backend: accelerator_backend(accelerator.kind).to_owned(),
                    utilization_percent: accelerator.utilization_percent,
                    used_vram_bytes: accelerator.used_vram_bytes,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAcceleratorUsage {
    pub name: String,
    pub backend: String,
    pub utilization_percent: Option<f32>,
    pub used_vram_bytes: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCredentialStatus {
    pub password_required: bool,
    pub password_saved: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMigrationOption {
    pub runtime_id: String,
    pub variant_id: Option<String>,
    pub available: bool,
    pub reason: String,
    pub requires_hugging_face_token: bool,
    pub token_saved: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMigrationReport {
    pub replacement: PersistedModelEntry,
    pub source_entry_id: String,
    pub source_can_be_removed: bool,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnostics {
    pub runtime_id: String,
    pub version: String,
    pub health: String,
    pub lifecycle: String,
    pub endpoint: Option<String>,
    pub effective_context_length: Option<u32>,
    pub effective_keep_alive: Option<String>,
    pub managed_container_engine: Option<String>,
    pub managed_container_name: Option<String>,
    pub managed_port: Option<u16>,
    pub recent_logs: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceSnapshot {
    pub model_id: String,
    pub state: String,
    pub sampled_at_unix_ms: u64,
    pub allocated_memory_bytes: u64,
    pub allocated_vram_bytes: u64,
    pub allocated_system_memory_bytes: u64,
    pub context_length: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceTestResult {
    pub benchmark_version: u32,
    pub tested_at: String,
    pub assessment: String,
    pub summary: String,
    pub first_token_millis: u64,
    pub first_visible_token_millis: Option<u64>,
    pub load_millis: u64,
    pub prompt_tokens_per_second: f64,
    pub generation_tokens_per_second: f64,
    pub prompt_token_count: u64,
    pub generated_token_count: u64,
    pub allocated_memory_bytes: u64,
    pub allocated_vram_bytes: u64,
    pub accelerator: String,
    pub effective_context_length: Option<u64>,
    pub expected_generation_tokens_per_second: Option<f64>,
    pub evidence: String,
    pub hardware_summary: String,
    pub workload: String,
    pub recommendation: Option<PerformanceTestRecommendation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceTestRecommendation {
    pub direction: String,
    pub model_id: String,
    pub model_name: String,
    pub runtime_model_id: String,
    pub estimated_first_visible_token_millis: Option<u64>,
    pub estimated_generation_tokens_per_second: f64,
    pub reason: String,
}

pub(crate) fn current_unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModelSummary {
    pub id: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSummary {
    pub revision: String,
    pub updated_at: String,
    pub model_count: usize,
    pub source: String,
    pub models: Vec<CatalogModelSummary>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub model_id: String,
    pub runtime_id: String,
    pub name: String,
    pub provider: String,
    pub description: String,
    pub version: String,
    pub size_bytes: u64,
    pub context_window: u32,
    pub runtime_digest: Option<String>,
    pub labels: Vec<String>,
    pub external_evaluations: Vec<ExternalEvaluationSummary>,
    pub estimated_loaded_memory_min_bytes: u64,
    pub estimated_loaded_memory_max_bytes: u64,
    pub fit: String,
    pub reasons: Vec<String>,
    pub recommended: bool,
    pub compatible: bool,
    pub license: LicenseSummary,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEvaluationSummary {
    pub publisher: String,
    pub leaderboard_name: String,
    pub source_model_name: String,
    pub overall_tier: String,
    pub notes: String,
    pub source_url: String,
    pub retrieved_at: String,
}

impl From<&ExternalEvaluation> for ExternalEvaluationSummary {
    fn from(evaluation: &ExternalEvaluation) -> Self {
        Self {
            publisher: evaluation.publisher.clone(),
            leaderboard_name: evaluation.leaderboard_name.clone(),
            source_model_name: evaluation.source_model_name.clone(),
            overall_tier: evaluation.overall_tier.as_str().to_owned(),
            notes: evaluation.notes.clone(),
            source_url: evaluation.source.url.clone(),
            retrieved_at: evaluation.source.retrieved_at.clone(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseSummary {
    pub profile_id: Option<String>,
    pub name: String,
    pub url: Option<String>,
    pub classification: String,
    pub commercial_use: String,
    pub redistribution: String,
    pub derivatives: String,
    pub requires_user_acceptance: bool,
    pub attribution: String,
    pub license_text: String,
    pub notice: String,
    pub ui_notice: String,
    pub summary: String,
    pub obligations: Vec<String>,
    pub restrictions: Vec<String>,
    pub geographic_restrictions: Vec<String>,
    pub usage_policy_url: Option<String>,
    pub reviewed_at: Option<String>,
}

impl From<&License> for LicenseSummary {
    fn from(license: &License) -> Self {
        Self {
            profile_id: license.profile_id.clone(),
            name: license.name.clone(),
            url: license.url.clone(),
            classification: license.classification.clone(),
            commercial_use: license.commercial_use.clone(),
            redistribution: license.redistribution.clone(),
            derivatives: license.derivatives.clone(),
            requires_user_acceptance: license.requires_user_acceptance,
            attribution: license.attribution.clone(),
            license_text: license.license_text.clone(),
            notice: license.notice.clone(),
            ui_notice: license.ui_notice.clone(),
            summary: license.summary.clone(),
            obligations: license.obligations.clone(),
            restrictions: license.restrictions.clone(),
            geographic_restrictions: license.geographic_restrictions.clone(),
            usage_policy_url: license.usage_policy_url.clone(),
            reviewed_at: license.reviewed_at.clone(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightCheck {
    pub id: String,
    pub status: String,
    pub message_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub can_install: bool,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub checks: Vec<PreflightCheck>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub model_id: String,
    pub phase: String,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_item: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_items: Option<u32>,
    pub message_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChatRequestOptions {
    pub system_prompt: Option<String>,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum ChatEvent {
    Delta {
        request_id: String,
        content: String,
    },
    Status {
        request_id: String,
        status: String,
        elapsed_ms: u64,
    },
    Done {
        request_id: String,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionResult {
    pub request_id: String,
    pub content: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub state: String,
    pub model_id: Option<String>,
    pub message: Option<String>,
}

impl RuntimeStatus {
    pub(crate) fn stopped() -> Self {
        Self {
            state: "stopped".to_owned(),
            model_id: None,
            message: None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDetails {
    pub base_url: String,
    pub chat_completions_url: String,
    pub completions_url: String,
    pub embeddings_url: String,
    pub model: String,
    pub api_key_required: bool,
    pub api_available: bool,
    pub chat_available: bool,
    pub embeddings_available: bool,
}
