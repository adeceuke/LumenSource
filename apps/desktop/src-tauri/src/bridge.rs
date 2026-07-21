//! Thin adaptation seam between Tauri and the shared Lumen Source crates.

use std::path::PathBuf;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use lumen_source_catalog::{
    Catalog, CatalogCache, CatalogLocation, CatalogService, CatalogSource, Ed25519Verifier,
    ModelVariant, Platform, ReqwestCatalogFetcher,
};
use lumen_source_hardware::{AcceleratorKind, HardwareFacts, HardwareProbe, PlatformHardwareProbe};
use lumen_source_host::{Host, LocalHost};
use lumen_source_recommend::{recommend, RecommendationRequest};
use lumen_source_runtime::{
    Artifact as RuntimeArtifact, ArtifactInstaller, OllamaRuntime, Runtime, RuntimeEndpoint,
    RuntimeProgress, RuntimeStatus as CoreRuntimeStatus, Url,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

const BUNDLED_CATALOG: &[u8] = include_bytes!("../../../../catalog/fixtures/catalog.v1.valid.json");
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

struct LoadedCatalog {
    catalog: Catalog,
    source: String,
}

#[derive(Default, Deserialize, Serialize)]
struct PersistedState {
    installed_model: Option<String>,
    runtime_executable: Option<PathBuf>,
    models: Vec<PersistedModelEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedModelEntry {
    pub id: String,
    pub name: String,
    pub model_id: String,
    pub model_name: String,
    pub version: String,
    pub location: String,
    pub running: bool,
    pub logs: Vec<String>,
}

pub struct SharedCoreAdapter {
    probe: Arc<PlatformHardwareProbe>,
    runtime: Arc<OllamaRuntime>,
    host: LocalHost<PlatformHardwareProbe, OllamaRuntime>,
    catalog: RwLock<Option<LoadedCatalog>>,
    installed_model: RwLock<Option<String>>,
    state_path: PathBuf,
}

impl SharedCoreAdapter {
    pub fn new() -> Result<Self, String> {
        let data_root = dirs::data_local_dir()
            .ok_or_else(|| "No local application data directory is available".to_owned())?;
        let state_path = data_root.join("lumen-source/state.json");
        let persisted = std::fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistedState>(&bytes).ok())
            .unwrap_or_default();
        let probe = Arc::new(PlatformHardwareProbe::default());
        let runtime = Arc::new(
            OllamaRuntime::new_with_executable(
                "http://127.0.0.1:11434",
                persisted
                    .runtime_executable
                    .unwrap_or_else(|| PathBuf::from("ollama")),
            )
            .map_err(|error| format!("Invalid Ollama endpoint: {error}"))?,
        );
        let host = LocalHost::new(Arc::clone(&probe), Arc::clone(&runtime));
        Ok(Self {
            probe,
            runtime,
            host,
            catalog: RwLock::new(None),
            installed_model: RwLock::new(persisted.installed_model),
            state_path,
        })
    }

    pub async fn detect_hardware(&self) -> Result<HardwareProfile, String> {
        let facts = self
            .probe
            .hardware_facts()
            .await
            .map_err(|error| error.to_string())?;
        Ok(HardwareProfile::from(&facts))
    }

    pub async fn load_catalog(&self, refresh: bool) -> Result<CatalogSummary, String> {
        if !refresh {
            if let Some(loaded) = self.catalog.read().await.as_ref() {
                return Ok(summary(loaded));
            }
        }

        let loaded = if refresh {
            match self.load_remote_catalog().await {
                Ok(remote) => remote,
                Err(error) if self.catalog.read().await.is_some() => {
                    return self.catalog.read().await.as_ref().map(summary).ok_or(error);
                }
                Err(_) => bundled_catalog()?,
            }
        } else {
            bundled_catalog()?
        };
        let result = summary(&loaded);
        *self.catalog.write().await = Some(loaded);
        Ok(result)
    }

    async fn load_remote_catalog(&self) -> Result<LoadedCatalog, String> {
        let catalog_url = std::env::var("LUMEN_SOURCE_CATALOG_URL")
            .map_err(|_| "LUMEN_SOURCE_CATALOG_URL is not configured".to_owned())?;
        let signature_url = std::env::var("LUMEN_SOURCE_CATALOG_SIGNATURE_URL")
            .map_err(|_| "LUMEN_SOURCE_CATALOG_SIGNATURE_URL is not configured".to_owned())?;
        let encoded_key = std::env::var("LUMEN_SOURCE_CATALOG_PUBLIC_KEY")
            .map_err(|_| "LUMEN_SOURCE_CATALOG_PUBLIC_KEY is not configured".to_owned())?;
        let key = STANDARD
            .decode(encoded_key.trim())
            .map_err(|error| format!("Catalog public key is not valid base64: {error}"))?;
        let key: [u8; 32] = key
            .try_into()
            .map_err(|_| "Catalog public key must contain exactly 32 bytes".to_owned())?;
        let verifier =
            Ed25519Verifier::from_public_key_bytes(&key).map_err(|error| error.to_string())?;
        let cache_root =
            dirs::cache_dir().ok_or_else(|| "No user cache directory is available".to_owned())?;
        let service = CatalogService::new(
            ReqwestCatalogFetcher::default(),
            verifier,
            CatalogCache::new(cache_root.join("lumen-source/catalog-v1.json")),
        );
        let (catalog, source) = service
            .load(&CatalogLocation::new(catalog_url, signature_url))
            .await
            .map_err(|error| error.to_string())?;
        Ok(LoadedCatalog {
            catalog,
            source: match source {
                CatalogSource::Remote => "network",
                CatalogSource::Cache => "cache",
            }
            .to_owned(),
        })
    }

    pub async fn recommendations(&self, intent: &str) -> Result<Vec<Recommendation>, String> {
        let catalog = self.catalog_clone().await?;
        let hardware = self
            .probe
            .hardware_facts()
            .await
            .map_err(|error| error.to_string())?;
        let request = RecommendationRequest {
            use_case: Some(intent_use_case(intent)?.to_owned()),
            priorities: Vec::new(),
            max_results: 5,
        };
        let report = recommend(&catalog, &hardware, &request);
        let mut mapped = Vec::new();
        for item in report.recommendations {
            let (model, variant) = find_variant(&catalog, &item.variant_id)?;
            mapped.push(Recommendation {
                model_id: variant.id.clone(),
                name: model.display_name.clone(),
                description: model.description.clone(),
                size_bytes: variant_size_bytes(variant),
                context_window: 32_768,
                fit: if item.score >= 60.0 {
                    "ideal"
                } else if item.score >= 35.0 {
                    "good"
                } else {
                    "limited"
                }
                .to_owned(),
                reasons: item.explanations,
            });
        }
        Ok(mapped)
    }

    pub async fn preflight(&self, variant_id: &str) -> Result<PreflightReport, String> {
        let catalog = self.catalog_clone().await?;
        let (_, variant) = find_variant(&catalog, variant_id)?;
        let facts = self
            .probe
            .hardware_facts()
            .await
            .map_err(|error| error.to_string())?;
        let required = variant_size_bytes(variant);
        let available = facts.storage.available_bytes;
        let runtime_available =
            self.runtime.health().await.is_ok() || self.runtime.executable_available().await;
        let runtime_installable = runtime_artifact(&catalog, &variant.runtime).is_ok();
        let storage_ok = available >= required;
        Ok(PreflightReport {
            can_install: storage_ok && (runtime_available || runtime_installable),
            required_bytes: required,
            available_bytes: available,
            checks: vec![
                check(
                    "hardware",
                    "Hardware compatibility",
                    "pass",
                    "The selected catalog variant matches this machine.",
                ),
                check(
                    "storage",
                    "Available storage",
                    if storage_ok { "pass" } else { "fail" },
                    if storage_ok {
                        "Enough free space is available."
                    } else {
                        "The model requires more free storage."
                    },
                ),
                check(
                    "runtime",
                    "Ollama runtime",
                    if runtime_available {
                        "pass"
                    } else if runtime_installable {
                        "warning"
                    } else {
                        "fail"
                    },
                    if runtime_available {
                        "Ollama is installed or already running."
                    } else if runtime_installable {
                        "A catalog-pinned Ollama archive will be downloaded and verified."
                    } else {
                        "No compatible verified runtime artifact is available."
                    },
                ),
            ],
        })
    }

    pub async fn install(&self, app: AppHandle, variant_id: String) -> Result<(), String> {
        let catalog = self.catalog_clone().await?;
        let (_, variant) = find_variant(&catalog, &variant_id)?;
        let runtime_ref = variant.runtime_ref.clone();
        let total = variant_size_bytes(variant);
        app.emit(
            "install-progress",
            progress(&variant_id, "preparing", 0, total, "Starting Ollama…"),
        )
        .map_err(|error| error.to_string())?;

        let progress_app = app.clone();
        let progress_model = variant_id.clone();
        let reporter = move |event| {
            let payload = runtime_progress(&progress_model, total, event);
            let _ = progress_app.emit("install-progress", payload);
        };

        if self.runtime.health().await.is_err() && !self.runtime.executable_available().await {
            let (artifact, version) = runtime_artifact(&catalog, &variant.runtime)?;
            let data_root = dirs::data_local_dir()
                .ok_or_else(|| "No local application data directory is available".to_owned())?;
            let install_dir = data_root
                .join("lumen-source/runtimes")
                .join(&variant.runtime)
                .join(version);
            let executable = ArtifactInstaller::default()
                .install_tar_zst(
                    &artifact,
                    &install_dir,
                    std::path::Path::new("bin/ollama"),
                    &reporter,
                )
                .await
                .map_err(|error| error.to_string())?;
            self.runtime.set_executable(executable).await;
        }
        self.runtime
            .ensure_running()
            .await
            .map_err(|error| error.to_string())?;

        self.host
            .install_model(&runtime_ref, &reporter)
            .await
            .map_err(|error| error.to_string())?;
        *self.installed_model.write().await = Some(runtime_ref);
        self.persist_state().await?;
        app.emit(
            "install-progress",
            progress(
                &variant_id,
                "complete",
                total,
                total,
                "Installation complete",
            ),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub async fn start(&self, variant_id: String) -> Result<RuntimeStatus, String> {
        let catalog = self.catalog_clone().await?;
        let (_, variant) = find_variant(&catalog, &variant_id)?;
        self.runtime
            .ensure_running()
            .await
            .map_err(|error| error.to_string())?;
        self.host
            .start(&variant.runtime_ref)
            .await
            .map_err(|error| error.to_string())?;
        *self.installed_model.write().await = Some(variant.runtime_ref.clone());
        self.persist_state().await?;
        Ok(RuntimeStatus {
            state: "running".to_owned(),
            model_id: Some(variant.runtime_ref.clone()),
            message: Some("Listening on localhost".to_owned()),
        })
    }

    pub async fn stop(&self) -> Result<RuntimeStatus, String> {
        let model = self
            .installed_model
            .read()
            .await
            .clone()
            .ok_or_else(|| "No model is currently selected".to_owned())?;
        self.host
            .stop(&model)
            .await
            .map_err(|error| error.to_string())?;
        Ok(RuntimeStatus::stopped())
    }

    pub async fn status(&self) -> Result<RuntimeStatus, String> {
        let status = self
            .host
            .status()
            .await
            .map_err(|error| error.to_string())?;
        Ok(match status.runtime {
            CoreRuntimeStatus::Unavailable | CoreRuntimeStatus::Idle => RuntimeStatus::stopped(),
            CoreRuntimeStatus::Running { models } => RuntimeStatus {
                state: "running".to_owned(),
                model_id: models.first().cloned(),
                message: Some("Ollama is serving a model".to_owned()),
            },
        })
    }

    pub async fn endpoint(&self) -> Result<EndpointDetails, String> {
        let model = self
            .installed_model
            .read()
            .await
            .clone()
            .ok_or_else(|| "Start a model before requesting endpoint details".to_owned())?;
        endpoint_details(self.runtime.endpoint(), model)
    }

    pub async fn load_models(&self) -> Result<Vec<PersistedModelEntry>, String> {
        let state = self.read_state().await?;
        Ok(state.models)
    }

    pub async fn save_models(&self, models: Vec<PersistedModelEntry>) -> Result<(), String> {
        let mut state = self.read_state().await?;
        state.models = models;
        self.write_state(&state).await
    }

    async fn catalog_clone(&self) -> Result<Catalog, String> {
        if self.catalog.read().await.is_none() {
            self.load_catalog(false).await?;
        }
        self.catalog
            .read()
            .await
            .as_ref()
            .map(|loaded| loaded.catalog.clone())
            .ok_or_else(|| "Catalog is unavailable".to_owned())
    }

    async fn persist_state(&self) -> Result<(), String> {
        let state = PersistedState {
            installed_model: self.installed_model.read().await.clone(),
            runtime_executable: Some(self.runtime.executable_path().await),
            models: self.read_state().await?.models,
        };
        self.write_state(&state).await
    }

    async fn read_state(&self) -> Result<PersistedState, String> {
        let persisted = std::fs::read(&self.state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistedState>(&bytes).ok())
            .unwrap_or_default();
        Ok(persisted)
    }

    async fn write_state(&self, state: &PersistedState) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
        let parent = self
            .state_path
            .parent()
            .ok_or_else(|| "State file has no parent directory".to_owned())?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
        let temporary = self.state_path.with_extension("json.tmp");
        tokio::fs::write(&temporary, bytes)
            .await
            .map_err(|error| error.to_string())?;
        tokio::fs::rename(&temporary, &self.state_path)
            .await
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    pub cpu: String,
    pub cpu_cores: usize,
    pub memory_bytes: u64,
    pub gpu: Option<GpuProfile>,
    pub platform: String,
    pub storage: StorageProfile,
}

impl From<&HardwareFacts> for HardwareProfile {
    fn from(facts: &HardwareFacts) -> Self {
        let gpu = facts.accelerators.first().map(|device| GpuProfile {
            name: device.name.clone(),
            memory_bytes: device.total_vram_bytes,
            backend: match device.kind {
                AcceleratorKind::Nvidia => "cuda",
                AcceleratorKind::Amd => "rocm",
                AcceleratorKind::Intel => "intel",
                AcceleratorKind::Other => "other",
            }
            .to_owned(),
        });
        Self {
            cpu: facts
                .cpu
                .model
                .clone()
                .unwrap_or_else(|| facts.cpu.architecture.clone()),
            cpu_cores: facts.cpu.logical_cores,
            memory_bytes: facts.total_ram_bytes,
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
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    pub context_window: u32,
    pub fit: String,
    pub reasons: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightCheck {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
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
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub state: String,
    pub model_id: Option<String>,
    pub message: Option<String>,
}

impl RuntimeStatus {
    fn stopped() -> Self {
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
    pub model: String,
    pub api_key_required: bool,
}

fn bundled_catalog() -> Result<LoadedCatalog, String> {
    Ok(LoadedCatalog {
        catalog: Catalog::from_slice(BUNDLED_CATALOG).map_err(|error| error.to_string())?,
        source: "bundled".to_owned(),
    })
}

fn summary(loaded: &LoadedCatalog) -> CatalogSummary {
    CatalogSummary {
        revision: loaded.catalog.catalog_version.clone(),
        updated_at: loaded.catalog.published_at.clone(),
        model_count: loaded.catalog.models.len(),
        source: loaded.source.clone(),
        models: loaded
            .catalog
            .models
            .iter()
            .filter_map(|model| {
                model.variants.first().and_then(|variant| {
                    loaded.catalog.runtimes.iter().find(|runtime| runtime.id == variant.runtime).map(
                        |runtime| CatalogModelSummary {
                            id: model.id.clone(),
                            display_name: model.display_name.clone(),
                            version: runtime.install.version.clone(),
                            description: model.description.clone(),
                        },
                    )
                })
            })
            .collect(),
    }
}

fn intent_use_case(intent: &str) -> Result<&'static str, String> {
    match intent {
        "chat" => Ok("general"),
        "code" => Ok("programming"),
        "creative" => Ok("writing"),
        _ => Err("Choose a supported use before requesting recommendations".to_owned()),
    }
}

fn find_variant<'a>(
    catalog: &'a Catalog,
    variant_id: &str,
) -> Result<(&'a lumen_source_catalog::ModelEntry, &'a ModelVariant), String> {
    catalog
        .models
        .iter()
        .find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| variant.id == variant_id)
                .map(|variant| (model, variant))
        })
        .ok_or_else(|| format!("Model variant `{variant_id}` is not in the active catalog"))
}

fn runtime_artifact(
    catalog: &Catalog,
    runtime_id: &str,
) -> Result<(RuntimeArtifact, String), String> {
    let runtime = catalog
        .runtimes
        .iter()
        .find(|runtime| runtime.id == runtime_id)
        .ok_or_else(|| format!("Runtime `{runtime_id}` is not in the active catalog"))?;
    let platform = current_platform()?;
    let raw_url = runtime
        .install
        .urls_by_platform
        .get(&platform)
        .ok_or_else(|| format!("Runtime `{runtime_id}` has no artifact for this platform"))?;
    let sha256 = runtime
        .install
        .sha256_by_platform
        .get(&platform)
        .ok_or_else(|| format!("Runtime `{runtime_id}` has no checksum for this platform"))?;
    let url = Url::parse(raw_url).map_err(|error| format!("Invalid runtime URL: {error}"))?;
    let executable_name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Runtime artifact URL has no filename".to_owned())?
        .to_owned();
    if !executable_name.ends_with(".tar.zst") {
        return Err(format!(
            "Runtime archive `{executable_name}` is not supported by this v0.1 platform adapter"
        ));
    }
    Ok((
        RuntimeArtifact {
            url,
            sha256: sha256.clone(),
            executable_name,
        },
        runtime.install.version.clone(),
    ))
}

fn current_platform() -> Result<Platform, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(Platform::LinuxX86_64),
        ("linux", "aarch64") => Ok(Platform::LinuxAarch64),
        ("macos", "aarch64") => Ok(Platform::DarwinArm64),
        ("windows", "x86_64") => Ok(Platform::WindowsX86_64),
        (os, arch) => Err(format!("Unsupported platform: {os}-{arch}")),
    }
}

fn variant_size_bytes(variant: &ModelVariant) -> u64 {
    variant
        .artifact
        .as_ref()
        .and_then(|artifact| artifact.size_bytes)
        .unwrap_or((variant.requirements.min_storage_gb * GIB) as u64)
}

fn check(id: &str, label: &str, status: &str, detail: &str) -> PreflightCheck {
    PreflightCheck {
        id: id.to_owned(),
        label: label.to_owned(),
        status: status.to_owned(),
        detail: detail.to_owned(),
    }
}

fn progress(
    model_id: &str,
    phase: &str,
    completed: u64,
    total: u64,
    message: &str,
) -> InstallProgress {
    InstallProgress {
        model_id: model_id.to_owned(),
        phase: phase.to_owned(),
        completed_bytes: completed,
        total_bytes: total,
        message: message.to_owned(),
    }
}

fn runtime_progress(
    model_id: &str,
    fallback_total: u64,
    event: RuntimeProgress,
) -> InstallProgress {
    match event {
        RuntimeProgress::Downloading { downloaded, total } => progress(
            model_id,
            "downloading",
            downloaded,
            total.unwrap_or(fallback_total),
            "Downloading runtime…",
        ),
        RuntimeProgress::Verifying => progress(
            model_id,
            "verifying",
            fallback_total,
            fallback_total,
            "Verifying download…",
        ),
        RuntimeProgress::Installing => progress(
            model_id,
            "installing",
            fallback_total,
            fallback_total,
            "Installing runtime…",
        ),
        RuntimeProgress::PullingModel {
            status,
            completed,
            total,
        } => progress(
            model_id,
            "downloading",
            completed.unwrap_or_default(),
            total.unwrap_or(fallback_total),
            &status,
        ),
        RuntimeProgress::Ready => progress(
            model_id,
            "installing",
            fallback_total,
            fallback_total,
            "Registering model…",
        ),
    }
}

fn endpoint_details(endpoint: RuntimeEndpoint, model: String) -> Result<EndpointDetails, String> {
    let base = endpoint.base_url.as_str().trim_end_matches('/');
    if base.is_empty() {
        return Err("Runtime endpoint is empty".to_owned());
    }
    Ok(EndpointDetails {
        base_url: format!("{base}/v1"),
        chat_completions_url: format!("{base}/v1/chat/completions"),
        model,
        api_key_required: false,
    })
}
