//! Thin adaptation seam between Tauri and the shared Lumen Source crates.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use lumen_source_catalog::{
    Catalog, CatalogCache, CatalogLocation, CatalogService, CatalogSource, Ed25519Verifier,
    ModelEntry, ModelVariant, Platform, ReqwestCatalogFetcher,
};
use lumen_source_hardware::{HardwareFacts, HardwareProbe, PlatformHardwareProbe};
use lumen_source_host::{Host, LocalHost};
use lumen_source_recommend::{recommend, RecommendationRequest};
use lumen_source_runtime::{
    Artifact as RuntimeArtifact, ArtifactInstaller, CancellationToken, ChatMessage, ChatProgress,
    DummyRuntime, OllamaRuntime, Runtime, RuntimeEndpoint, RuntimeError, RuntimeProgress,
    RuntimeStatus as CoreRuntimeStatus, Url,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;

pub use crate::bridge_types::*;
use crate::credential_store;
use crate::model_reconciliation::{
    reconcile_models, reconcile_unavailable_models, same_ollama_reference, with_remote_models,
    DUMMY_RUNTIME,
};
use crate::remote::{
    connect as connect_remote, probe_hardware as probe_remote_hardware,
    probe_usage as probe_remote_usage, RemoteAuthentication, RemoteConnectionReport, RemoteSession,
    RemoteTargetConfig, RemoteTargetProfile,
};
use crate::settings::{
    migrate_settings, validate_settings, ApplicationSettings, OllamaConnectionReport,
    SettingsSaveReport,
};
use crate::telemetry::{failure_category, memory_tier, ChatOutcome, Telemetry, TelemetryEvent};

const BUNDLED_CATALOG: &[u8] = include_bytes!("../../../../catalog/model-list.json");
const PRODUCTION_CATALOG_URL: &str = "https://lumensource.dev/v2/model-list.json";
const PRODUCTION_CATALOG_SIGNATURE_URL: &str = "https://lumensource.dev/v2/model-list.json.sig";
const PRODUCTION_CATALOG_PUBLIC_KEY: &str = "r3ICuFyaSuGGQwO/xKO6sxjEiJJHAqjO+FSknV583q0=";
#[cfg(test)]
const TEST_CATALOG: &[u8] = include_bytes!("../../../../catalog/fixtures/catalog.v1.valid.json");
#[cfg(debug_assertions)]
const DEVELOPMENT_CATALOG: &[u8] =
    include_bytes!("../../../../catalog/fixtures/catalog.v1.valid.json");
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const OLLAMA_RUNTIME: &str = "ollama";

struct LoadedCatalog {
    catalog: Catalog,
    source: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct PersistedState {
    installed_model: Option<String>,
    #[serde(default)]
    selected_runtime: Option<String>,
    runtime_executable: Option<PathBuf>,
    #[serde(default)]
    settings: ApplicationSettings,
    #[serde(default)]
    remote_targets: Vec<RemoteTargetConfig>,
    #[serde(default)]
    models: Vec<PersistedModelEntry>,
}

struct ActiveInstall {
    variant_id: String,
    cancellation: CancellationToken,
}

struct ActiveChat {
    runtime_model_id: String,
    cancellation: CancellationToken,
}

#[derive(Default)]
struct PullItemTracker {
    digests: Vec<String>,
}

impl PullItemTracker {
    fn observe(&mut self, digest: &str) -> u32 {
        if let Some(index) = self.digests.iter().position(|known| known == digest) {
            return index as u32 + 1;
        }
        self.digests.push(digest.to_owned());
        self.digests.len() as u32
    }
}

pub struct SharedCoreAdapter {
    probe: Arc<PlatformHardwareProbe>,
    runtime: Arc<OllamaRuntime>,
    dummy_runtime: Arc<DummyRuntime>,
    host: LocalHost<PlatformHardwareProbe, OllamaRuntime>,
    catalog: RwLock<Option<LoadedCatalog>>,
    installed_model: RwLock<Option<String>>,
    selected_runtime: RwLock<Option<String>>,
    state: RwLock<PersistedState>,
    state_write: Mutex<()>,
    active_install: Mutex<Option<ActiveInstall>>,
    active_chat: Mutex<Option<ActiveChat>>,
    remote_session: RwLock<Option<Arc<RemoteSession>>>,
    state_path: PathBuf,
    telemetry: Telemetry,
}

pub struct InstallOptions {
    pub license_basis: String,
    pub license_reference: Option<String>,
    pub license_acknowledged: bool,
    pub install_runtime: bool,
}

impl SharedCoreAdapter {
    pub fn new() -> Result<Self, String> {
        let data_root = dirs::data_local_dir()
            .ok_or_else(|| "No local application data directory is available".to_owned())?;
        let state_path = data_root.join("lumen-source/state.json");
        let telemetry = Telemetry::new(&data_root);
        let mut persisted = std::fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistedState>(&bytes).ok())
            .unwrap_or_default();
        persisted.settings = migrate_settings(persisted.settings);
        if persisted.settings.ollama.executable_path.is_none() {
            persisted.settings.ollama.executable_path = persisted.runtime_executable.clone();
        }
        let probe = Arc::new(PlatformHardwareProbe::default());
        let runtime = Arc::new(
            OllamaRuntime::new_configured(
                &persisted.settings.ollama.endpoint,
                persisted
                    .settings
                    .ollama
                    .executable_path
                    .as_deref()
                    .map(resolve_ollama_executable)
                    .unwrap_or_else(default_ollama_executable),
                persisted
                    .settings
                    .ollama
                    .server_environment(persisted.settings.storage.model_directory.as_deref()),
            )
            .map_err(|error| format!("Invalid Ollama endpoint: {error}"))?,
        );
        let dummy_runtime = Arc::new(
            DummyRuntime::new("http://127.0.0.1:9999")
                .map_err(|error| format!("Invalid dummy endpoint: {error}"))?,
        );
        let host = LocalHost::new(Arc::clone(&probe), Arc::clone(&runtime));
        Ok(Self {
            probe,
            runtime,
            dummy_runtime,
            host,
            catalog: RwLock::new(None),
            installed_model: RwLock::new(persisted.installed_model.clone()),
            selected_runtime: RwLock::new(persisted.selected_runtime.clone()),
            state: RwLock::new(persisted),
            state_write: Mutex::new(()),
            active_install: Mutex::new(None),
            active_chat: Mutex::new(None),
            remote_session: RwLock::new(None),
            state_path,
            telemetry,
        })
    }

    pub async fn telemetry_preference(&self) -> Result<Option<bool>, String> {
        self.telemetry.preference().await
    }

    pub async fn settings(&self) -> Result<ApplicationSettings, String> {
        let mut settings = self.state.read().await.settings.clone();
        if settings.ollama.executable_path.is_none() {
            settings.ollama.executable_path = Some(self.runtime.executable_path().await);
        }
        if let Some(enabled) = self.telemetry.preference().await? {
            settings.privacy.telemetry_enabled = enabled;
        }
        Ok(settings)
    }

    pub async fn save_settings(
        &self,
        settings: ApplicationSettings,
        confirm_network_exposure: bool,
    ) -> Result<SettingsSaveReport, String> {
        let settings = migrate_settings(settings);
        let validation_errors = validate_settings(&settings);
        if !validation_errors.is_empty() {
            return Err(validation_errors
                .into_iter()
                .map(|error| format!("{}: {}", error.field, error.message))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        if settings.ollama.exposes_network() && !confirm_network_exposure {
            return Err(
                "Network exposure confirmation is required before Ollama can bind beyond loopback."
                    .to_owned(),
            );
        }
        let previous = self.state.read().await.settings.clone();
        let runtime_restart_required = previous.ollama != settings.ollama;
        self.telemetry
            .set_enabled(settings.privacy.telemetry_enabled)
            .await?;
        if settings.privacy.telemetry_enabled {
            self.telemetry.retry_upload();
        }
        {
            let mut state = self.state.write().await;
            state.settings = settings.clone();
            state.runtime_executable = settings.ollama.executable_path.clone();
        }
        self.runtime
            .set_endpoint(&settings.ollama.endpoint)
            .map_err(|error| error.to_string())?;
        self.runtime
            .set_executable(
                settings
                    .ollama
                    .executable_path
                    .as_deref()
                    .map(resolve_ollama_executable)
                    .unwrap_or_else(default_ollama_executable),
            )
            .await;
        self.runtime
            .set_server_environment(
                settings
                    .ollama
                    .server_environment(settings.storage.model_directory.as_deref()),
            )
            .await;
        self.flush_state().await?;
        let mut returned_settings = settings;
        if returned_settings.ollama.executable_path.is_none() {
            returned_settings.ollama.executable_path = Some(self.runtime.executable_path().await);
        }
        Ok(SettingsSaveReport {
            settings: returned_settings,
            runtime_restart_required,
        })
    }

    pub async fn reset_settings(&self) -> Result<SettingsSaveReport, String> {
        self.save_settings(ApplicationSettings::default(), false)
            .await
    }

    pub async fn test_ollama_connection(
        &self,
        settings: ApplicationSettings,
    ) -> OllamaConnectionReport {
        let endpoint = settings.ollama.endpoint.clone();
        let runtime = match OllamaRuntime::new(&endpoint) {
            Ok(runtime) => runtime,
            Err(error) => {
                return OllamaConnectionReport {
                    healthy: false,
                    endpoint,
                    version: None,
                    message: error.to_string(),
                };
            }
        };
        match runtime.version().await {
            Ok(version) => OllamaConnectionReport {
                healthy: true,
                endpoint,
                version: Some(version.clone()),
                message: format!("Connected to Ollama {version}."),
            },
            Err(error) => OllamaConnectionReport {
                healthy: false,
                endpoint,
                version: None,
                message: error.to_string(),
            },
        }
    }

    pub async fn restart_managed_ollama(&self) -> Result<OllamaConnectionReport, String> {
        let settings = self.settings().await?;
        self.runtime
            .restart_managed_server()
            .await
            .map_err(|error| error.to_string())?;
        Ok(self.test_ollama_connection(settings).await)
    }

    pub async fn set_telemetry_enabled(&self, enabled: bool) -> Result<(), String> {
        self.telemetry.set_enabled(enabled).await?;
        if enabled {
            self.telemetry.retry_upload();
        }
        Ok(())
    }

    pub fn retry_telemetry_upload(&self) {
        self.telemetry.retry_upload();
    }

    pub async fn remote_targets(&self) -> Vec<RemoteTargetProfile> {
        let mut targets = self
            .state
            .read()
            .await
            .remote_targets
            .iter()
            .cloned()
            .map(RemoteTargetProfile::from)
            .collect::<Vec<_>>();
        targets.sort_by_key(|target| target.target_name.to_lowercase());
        targets
    }

    pub async fn save_remote_target(
        &self,
        config: RemoteTargetConfig,
    ) -> Result<RemoteTargetProfile, String> {
        let config = config.normalized();
        config.validate()?;
        let profile = RemoteTargetProfile::from(config.clone());
        {
            let mut state = self.state.write().await;
            state
                .remote_targets
                .retain(|target| target.target_id() != profile.target_id);
            state.remote_targets.push(config);
        }
        self.flush_state().await?;
        Ok(profile)
    }

    pub async fn check_remote_target(
        &self,
        config: RemoteTargetConfig,
        password: Option<Zeroizing<String>>,
    ) -> Result<RemoteConnectionReport, String> {
        let config = config.normalized();
        let password = self.password_for_config(&config, password).await?;
        let attempt = connect_remote(config.clone(), password).await?;
        if let Some(session) = attempt.session {
            let target_id = config.target_id();
            {
                let mut state = self.state.write().await;
                state
                    .remote_targets
                    .retain(|target| target.target_id() != target_id);
                state.remote_targets.push(config);
            }
            *self.remote_session.write().await = Some(session);
            self.flush_state().await?;
        }
        Ok(attempt.report)
    }

    async fn runtime_for_target(&self, target_id: &str) -> Result<Arc<OllamaRuntime>, String> {
        self.runtime_for_target_with_password(target_id, None).await
    }

    async fn runtime_for_target_with_password(
        &self,
        target_id: &str,
        password: Option<Zeroizing<String>>,
    ) -> Result<Arc<OllamaRuntime>, String> {
        if target_id == "local" {
            return Ok(Arc::clone(&self.runtime));
        }
        if let Some(session) = self.remote_session.read().await.clone() {
            if session.target_id() == target_id && session.healthy().await {
                return Ok(Arc::clone(&session.runtime));
            }
        }
        let config = self.remote_config(target_id).await?;
        let password = self.password_for_config(&config, password).await?;
        let attempt = connect_remote(config, password).await?;
        let Some(session) = attempt.session else {
            let detail = attempt
                .report
                .checks
                .iter()
                .filter(|check| check.status == "fail")
                .map(|check| {
                    check.guidance.as_deref().map_or_else(
                        || check.detail.clone(),
                        |guidance| format!("{} {guidance}", check.detail),
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            return Err(if detail.is_empty() {
                "The remote target is unavailable".to_owned()
            } else {
                detail
            });
        };
        let runtime = Arc::clone(&session.runtime);
        *self.remote_session.write().await = Some(session);
        Ok(runtime)
    }

    async fn hardware_for_target(&self, target_id: &str) -> Result<HardwareFacts, String> {
        if target_id == "local" {
            return self
                .probe
                .hardware_facts()
                .await
                .map_err(|error| error.to_string());
        }
        if let Some(hardware) = self
            .remote_session
            .read()
            .await
            .as_ref()
            .filter(|session| session.target_id() == target_id)
            .map(|session| session.hardware.clone())
        {
            return Ok(hardware);
        }
        let config = self.remote_config(target_id).await?;
        let password = self.password_for_config(&config, None).await?;
        probe_remote_hardware(&config, password).await
    }

    async fn remote_config(&self, target_id: &str) -> Result<RemoteTargetConfig, String> {
        self.state
            .read()
            .await
            .remote_targets
            .iter()
            .find(|target| target.target_id() == target_id)
            .cloned()
            .ok_or_else(|| format!("Remote target `{target_id}` is not configured"))
    }

    async fn password_for_config(
        &self,
        config: &RemoteTargetConfig,
        provided: Option<Zeroizing<String>>,
    ) -> Result<Option<Zeroizing<String>>, String> {
        if config.authentication != RemoteAuthentication::Password {
            return Ok(None);
        }
        if let Some(password) = provided.filter(|password| !password.is_empty()) {
            return Ok(Some(password));
        }
        match credential_store::load_password(config.target_id()).await {
            Ok(Some(password)) => Ok(Some(password)),
            Ok(None) => Err(
                "SSH_PASSWORD_REQUIRED: Enter the SSH password for this remote machine."
                    .to_owned(),
            ),
            Err(error) => Err(format!(
                "SSH_PASSWORD_REQUIRED: The saved SSH password is unavailable. Enter it to reconnect. {error}"
            )),
        }
    }

    pub async fn remote_credential_status(
        &self,
        target_id: &str,
    ) -> Result<RemoteCredentialStatus, String> {
        let config = self.remote_config(target_id).await?;
        let password_required = config.authentication == RemoteAuthentication::Password;
        let password_saved = if password_required {
            credential_store::password_is_saved(target_id.to_owned()).await?
        } else {
            false
        };
        Ok(RemoteCredentialStatus {
            password_required,
            password_saved,
        })
    }

    pub async fn save_remote_password(
        &self,
        target_id: &str,
        password: Zeroizing<String>,
    ) -> Result<(), String> {
        let config = self.remote_config(target_id).await?;
        if config.authentication != RemoteAuthentication::Password {
            return Err("This remote machine does not use password authentication.".to_owned());
        }
        if password.is_empty() {
            return Err("Enter an SSH password before saving it.".to_owned());
        }
        credential_store::save_password(target_id.to_owned(), password).await
    }

    pub async fn delete_remote_password(&self, target_id: &str) -> Result<(), String> {
        self.remote_config(target_id).await?;
        credential_store::delete_password(target_id.to_owned()).await
    }

    pub async fn detect_hardware(
        &self,
        target_id: &str,
        password: Option<Zeroizing<String>>,
    ) -> Result<HardwareProfile, String> {
        let facts = if target_id == "local" || password.is_none() {
            self.hardware_for_target(target_id).await?
        } else {
            let config = self.remote_config(target_id).await?;
            let password = self.password_for_config(&config, password).await?;
            probe_remote_hardware(&config, password).await?
        };
        let profile = HardwareProfile::from(&facts);
        if target_id == "local" {
            let vram_tier = facts
                .accelerators
                .iter()
                .filter_map(|accelerator| accelerator.total_vram_bytes)
                .max()
                .map(memory_tier)
                .unwrap_or_else(|| "none".to_owned());
            self.telemetry.record(TelemetryEvent::Hardware {
                ram_tier: memory_tier(facts.total_ram_bytes),
                vram_tier,
                accelerator: facts
                    .accelerators
                    .first()
                    .map(|accelerator| accelerator_backend(accelerator.kind).to_owned())
                    .unwrap_or_else(|| "cpu-only".to_owned()),
            });
        }
        Ok(profile)
    }

    pub async fn machine_usage(
        &self,
        target_id: &str,
        password: Option<Zeroizing<String>>,
    ) -> Result<MachineUsageSnapshot, String> {
        let usage = if target_id == "local" {
            self.probe
                .usage_snapshot()
                .await
                .map_err(|error| error.to_string())?
        } else {
            let config = self.remote_config(target_id).await?;
            let password = self.password_for_config(&config, password).await?;
            probe_remote_usage(&config, password).await?
        };
        Ok(MachineUsageSnapshot::from_usage(target_id, usage))
    }

    pub async fn load_catalog(&self, refresh: bool) -> Result<CatalogSummary, String> {
        if !refresh {
            if let Some(loaded) = self.catalog.read().await.as_ref() {
                return Ok(summary(loaded));
            }
        }

        let mut loaded = match self.load_remote_catalog().await {
            Ok(remote) => remote,
            Err(error) if self.catalog.read().await.is_some() => {
                return self.catalog.read().await.as_ref().map(summary).ok_or(error);
            }
            Err(_) => bundled_catalog()?,
        };
        add_development_catalog_entries(&mut loaded.catalog);
        let result = summary(&loaded);
        self.telemetry.record(TelemetryEvent::CatalogLoad {
            revision: result.revision.clone(),
            source: result.source.clone(),
        });
        *self.catalog.write().await = Some(loaded);
        Ok(result)
    }

    async fn load_remote_catalog(&self) -> Result<LoadedCatalog, String> {
        let catalog_url = std::env::var("LUMEN_SOURCE_CATALOG_URL")
            .unwrap_or_else(|_| PRODUCTION_CATALOG_URL.to_owned());
        let signature_url = std::env::var("LUMEN_SOURCE_CATALOG_SIGNATURE_URL")
            .unwrap_or_else(|_| PRODUCTION_CATALOG_SIGNATURE_URL.to_owned());
        let encoded_key = std::env::var("LUMEN_SOURCE_CATALOG_PUBLIC_KEY")
            .unwrap_or_else(|_| PRODUCTION_CATALOG_PUBLIC_KEY.to_owned());
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
            CatalogCache::new(cache_root.join("lumen-source/catalog-v2.json")),
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

    pub async fn recommendations(
        &self,
        intent: &str,
        target_id: &str,
    ) -> Result<Vec<Recommendation>, String> {
        let catalog = self.catalog_clone().await?;
        let hardware = self.hardware_for_target(target_id).await?;
        let request = RecommendationRequest {
            use_case: Some(intent_use_case(intent)?.to_owned()),
            priorities: Vec::new(),
            max_results: 0,
        };
        let report = recommend(&catalog, &hardware, &request);
        let supports_runtime = |runtime: &str| {
            runtime == OLLAMA_RUNTIME || (target_id == "local" && runtime == DUMMY_RUNTIME)
        };
        let recommended_variant_id = report
            .recommendations
            .iter()
            .find(|item| supports_runtime(&item.runtime_id))
            .map(|item| item.variant_id.clone());
        let mut mapped = Vec::new();
        for item in report
            .recommendations
            .into_iter()
            .filter(|item| supports_runtime(&item.runtime_id))
        {
            let (model, variant) = find_variant(&catalog, &item.variant_id)?;
            let runtime = catalog
                .runtimes
                .iter()
                .find(|runtime| runtime.id == variant.runtime)
                .ok_or_else(|| {
                    format!("Runtime `{}` is not in the active catalog", variant.runtime)
                })?;
            mapped.push(Recommendation {
                model_id: variant.id.clone(),
                runtime_id: variant.runtime.clone(),
                name: model.display_name.clone(),
                provider: model
                    .provider
                    .clone()
                    .unwrap_or_else(|| "Independent publisher".to_owned()),
                description: model.description.clone(),
                version: runtime.install.version.clone(),
                size_bytes: variant_size_bytes(variant),
                context_window: variant.context_window_tokens.unwrap_or(32_768),
                runtime_digest: variant.runtime_digest.clone(),
                fit: if item.score >= 60.0 {
                    "ideal"
                } else if item.score >= 35.0 {
                    "good"
                } else {
                    "limited"
                }
                .to_owned(),
                reasons: item.explanations,
                recommended: recommended_variant_id.as_deref() == Some(variant.id.as_str()),
                compatible: true,
                license: LicenseSummary::from(&model.license),
            });
        }

        for item in report.exclusions {
            let (model, variant) = find_variant(&catalog, &item.variant_id)?;
            if !supports_runtime(&variant.runtime) {
                continue;
            }
            let runtime = catalog
                .runtimes
                .iter()
                .find(|runtime| runtime.id == variant.runtime)
                .ok_or_else(|| {
                    format!("Runtime `{}` is not in the active catalog", variant.runtime)
                })?;
            mapped.push(Recommendation {
                model_id: variant.id.clone(),
                runtime_id: variant.runtime.clone(),
                name: model.display_name.clone(),
                provider: model
                    .provider
                    .clone()
                    .unwrap_or_else(|| "Independent publisher".to_owned()),
                description: model.description.clone(),
                version: runtime.install.version.clone(),
                size_bytes: variant_size_bytes(variant),
                context_window: variant.context_window_tokens.unwrap_or(32_768),
                runtime_digest: variant.runtime_digest.clone(),
                fit: "incompatible".to_owned(),
                reasons: item.reasons,
                recommended: false,
                compatible: false,
                license: LicenseSummary::from(&model.license),
            });
        }
        Ok(mapped)
    }

    pub async fn preflight(
        &self,
        variant_id: &str,
        target_id: &str,
    ) -> Result<PreflightReport, String> {
        let catalog = self.catalog_clone().await?;
        let (_, variant) = find_variant(&catalog, variant_id)?;
        ensure_supported_runtime(variant)?;
        let facts = self.hardware_for_target(target_id).await?;
        if target_id != "local" {
            if variant.runtime != OLLAMA_RUNTIME {
                return Err("Remote targets support Ollama catalog models only".to_owned());
            }
            let compatibility = recommend(&catalog, &facts, &RecommendationRequest::default());
            let hardware_reasons = compatibility
                .exclusions
                .iter()
                .find(|exclusion| exclusion.variant_id == variant_id)
                .map(|exclusion| {
                    exclusion
                        .reasons
                        .iter()
                        .filter(|reason| !reason.contains("free storage"))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let hardware_ok = hardware_reasons.is_empty();
            let hardware_detail = if hardware_ok {
                "The selected catalog variant matches the remote target hardware.".to_owned()
            } else {
                hardware_reasons.join(" ")
            };
            let required = (variant.requirements.min_storage_gb * GIB).ceil() as u64;
            let available = facts.storage.available_bytes;
            let storage_ok = available >= required;
            return Ok(PreflightReport {
                can_install: hardware_ok && storage_ok,
                required_bytes: required,
                available_bytes: available,
                checks: vec![
                    check("connection", "pass", "remote.remoteConnection", None),
                    check(
                        "hardware",
                        if hardware_ok { "pass" } else { "fail" },
                        if hardware_ok {
                            "remote.remoteHardwareCompatible"
                        } else {
                            "remote.hardwareIncompatible"
                        },
                        (!hardware_ok).then_some(hardware_detail.as_str()),
                    ),
                    check(
                        "storage",
                        if storage_ok { "pass" } else { "fail" },
                        if storage_ok {
                            "remote.remoteStorageEnough"
                        } else {
                            "remote.remoteStorageInsufficient"
                        },
                        None,
                    ),
                    check("runtime", "pass", "remote.remoteRuntime", None),
                    check(
                        "source",
                        "pass",
                        "remote.source",
                        Some(variant.runtime_ref.as_str()),
                    ),
                ],
            });
        }
        let compatibility = recommend(&catalog, &facts, &RecommendationRequest::default());
        let hardware_reasons = compatibility
            .exclusions
            .iter()
            .find(|exclusion| exclusion.variant_id == variant_id)
            .map(|exclusion| {
                exclusion
                    .reasons
                    .iter()
                    .filter(|reason| !reason.contains("free storage"))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let hardware_ok = hardware_reasons.is_empty();
        let hardware_detail = if hardware_ok {
            "The selected catalog variant matches this machine.".to_owned()
        } else {
            hardware_reasons.join(" ")
        };
        let required = (variant.requirements.min_storage_gb * GIB).ceil() as u64;
        let available = facts.storage.available_bytes;
        let is_dummy = variant.runtime == DUMMY_RUNTIME;
        let runtime_available = is_dummy
            || self.runtime.health().await.is_ok()
            || self.runtime.executable_available().await;
        let runtime_artifact = (!is_dummy).then(|| runtime_artifact(&catalog, &variant.runtime));
        let runtime_installable = runtime_artifact.as_ref().is_some_and(Result::is_ok);
        let (runtime_key, runtime_detail) = if is_dummy {
            ("local.runtimeDummy", None)
        } else if runtime_available {
            ("local.runtimeAvailable", None)
        } else if let Some(Ok((artifact, _))) = &runtime_artifact {
            (
                "local.runtimeInstallable",
                Some(format!("{}\t{}", artifact.url, artifact.sha256)),
            )
        } else {
            ("local.runtimeUnavailable", None)
        };
        let storage_ok = available >= required;
        Ok(PreflightReport {
            can_install: hardware_ok && storage_ok && (runtime_available || runtime_installable),
            required_bytes: required,
            available_bytes: available,
            checks: vec![
                check(
                    "hardware",
                    if hardware_ok { "pass" } else { "fail" },
                    if hardware_ok {
                        "local.hardwareCompatible"
                    } else {
                        "local.hardwareIncompatible"
                    },
                    (!hardware_ok).then_some(hardware_detail.as_str()),
                ),
                check(
                    "storage",
                    if storage_ok { "pass" } else { "fail" },
                    if storage_ok {
                        "local.storageEnough"
                    } else {
                        "local.storageInsufficient"
                    },
                    None,
                ),
                check(
                    "runtime",
                    if runtime_available {
                        "pass"
                    } else if runtime_installable {
                        "warning"
                    } else {
                        "fail"
                    },
                    runtime_key,
                    runtime_detail.as_deref(),
                ),
                check(
                    "source",
                    "pass",
                    if is_dummy {
                        "local.sourceDummy"
                    } else {
                        "local.sourceOllama"
                    },
                    Some(variant.runtime_ref.as_str()),
                ),
            ],
        })
    }

    pub async fn install(
        &self,
        app: AppHandle,
        variant_id: String,
        target_id: String,
        options: InstallOptions,
    ) -> Result<(), String> {
        self.validate_license_authorization(
            &variant_id,
            &options.license_basis,
            options.license_reference.as_deref(),
            options.license_acknowledged,
        )
        .await?;
        let telemetry_model_id = {
            let catalog = self.catalog_clone().await?;
            let (model, _) = find_variant(&catalog, &variant_id)?;
            model.id.clone()
        };
        let cancellation = CancellationToken::new();
        {
            let mut active = self.active_install.lock().await;
            if let Some(install) = active.as_ref() {
                return Err(format!(
                    "Installation for `{}` is already in progress",
                    install.variant_id
                ));
            }
            *active = Some(ActiveInstall {
                variant_id: variant_id.clone(),
                cancellation: cancellation.clone(),
            });
        }

        let telemetry_variant_id = variant_id.clone();
        let result = self
            .install_inner(
                app,
                variant_id,
                &target_id,
                options.install_runtime,
                &cancellation,
            )
            .await;
        self.active_install.lock().await.take();
        self.telemetry.record(TelemetryEvent::ModelInstall {
            model_id: telemetry_model_id,
            variant_id: telemetry_variant_id,
            deployment: deployment_kind(&target_id),
            succeeded: result.is_ok(),
            failure: result.as_ref().err().map(|error| failure_category(error)),
        });
        result
    }

    async fn validate_license_authorization(
        &self,
        variant_id: &str,
        license_basis: &str,
        license_reference: Option<&str>,
        acknowledged: bool,
    ) -> Result<(), String> {
        let catalog = self.catalog_clone().await?;
        let (model, _) = find_variant(&catalog, variant_id)?;
        match license_basis {
            "catalog" if model.license.requires_user_acceptance && !acknowledged => Err(format!(
                "The {} must be acknowledged before installation",
                model.license.name
            )),
            "catalog" => Ok(()),
            "separate" if !acknowledged => {
                Err("Confirm that the separate license authorizes this use".to_owned())
            }
            "separate" if license_reference.is_none_or(|value| value.trim().is_empty()) => {
                Err("Enter a local reference for the separate license or authorization".to_owned())
            }
            "separate" => Ok(()),
            _ => Err(
                "Choose the cataloged terms or a separate license before installation".to_owned(),
            ),
        }
    }

    pub async fn cancel_install(&self) -> bool {
        let active = self.active_install.lock().await;
        if let Some(install) = active.as_ref() {
            install.cancellation.cancel();
            true
        } else {
            false
        }
    }

    async fn install_inner(
        &self,
        app: AppHandle,
        variant_id: String,
        target_id: &str,
        install_runtime: bool,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        let catalog = self.catalog_clone().await?;
        let (_, variant) = find_variant(&catalog, &variant_id)?;
        ensure_supported_runtime(variant)?;
        cancellation.check().map_err(|error| error.to_string())?;
        let runtime_ref = variant.runtime_ref.clone();
        let total = variant_size_bytes(variant);
        let total_items = variant.download_item_count;
        app.emit(
            "install-progress",
            progress(
                &variant_id,
                "preparing",
                0,
                total,
                None,
                None,
                if variant.runtime == DUMMY_RUNTIME {
                    "prepareDummy"
                } else if target_id != "local" {
                    "prepareRemote"
                } else {
                    "startOllama"
                },
            ),
        )
        .map_err(|error| error.to_string())?;

        let progress_app = app.clone();
        let progress_model = variant_id.clone();
        let pull_items = StdMutex::new(PullItemTracker::default());
        let reporter = move |event| {
            let payload = runtime_progress(&progress_model, total, total_items, &pull_items, event);
            let _ = progress_app.emit("install-progress", payload);
        };

        if target_id != "local" && variant.runtime != OLLAMA_RUNTIME {
            return Err("Remote targets support Ollama catalog models only".to_owned());
        }
        if variant.runtime == DUMMY_RUNTIME {
            self.dummy_runtime
                .pull_model_cancellable(&runtime_ref, &reporter, cancellation)
                .await
                .map_err(|error| error.to_string())?;
        } else if target_id == "local" {
            cancellation.check().map_err(|error| error.to_string())?;
            let runtime_healthy = tokio::select! {
                _ = cancellation.cancelled() => return Err("installation cancelled".to_owned()),
                result = self.runtime.health() => result.is_ok(),
            };
            let executable_available = if runtime_healthy {
                false
            } else {
                tokio::select! {
                    _ = cancellation.cancelled() => return Err("installation cancelled".to_owned()),
                    available = self.runtime.executable_available() => available,
                }
            };
            if !runtime_healthy && !executable_available {
                if !install_runtime {
                    return Err(
                        "Ollama is not installed. Select the option to install Ollama, or install it separately and retry."
                            .to_owned(),
                    );
                }
                let (artifact, version) = runtime_artifact(&catalog, &variant.runtime)?;
                let data_root = dirs::data_local_dir()
                    .ok_or_else(|| "No local application data directory is available".to_owned())?;
                let install_dir = data_root
                    .join("lumen-source/runtimes")
                    .join(&variant.runtime)
                    .join(version);
                let installer = ArtifactInstaller::default();
                let executable = if artifact.executable_name.ends_with(".zip") {
                    installer
                        .install_zip_cancellable(
                            &artifact,
                            &install_dir,
                            std::path::Path::new("ollama.exe"),
                            &reporter,
                            cancellation,
                        )
                        .await
                } else {
                    installer
                        .install_tar_zst_cancellable(
                            &artifact,
                            &install_dir,
                            std::path::Path::new("bin/ollama"),
                            &reporter,
                            cancellation,
                        )
                        .await
                }
                .map_err(|error| error.to_string())?;
                self.runtime.set_executable(executable).await;
            }
            cancellation.check().map_err(|error| error.to_string())?;
            self.runtime
                .ensure_running_cancellable(cancellation)
                .await
                .map_err(|error| error.to_string())?;

            cancellation.check().map_err(|error| error.to_string())?;
            self.runtime
                .pull_model_cancellable(&runtime_ref, &reporter, cancellation)
                .await
                .map_err(|error| error.to_string())?;
        } else {
            let runtime = self.runtime_for_target(target_id).await?;
            runtime
                .pull_model_cancellable(&runtime_ref, &reporter, cancellation)
                .await
                .map_err(|error| error.to_string())?;
        }
        cancellation.check().map_err(|error| error.to_string())?;
        *self.installed_model.write().await = Some(runtime_ref);
        *self.selected_runtime.write().await = Some(variant.runtime.clone());
        self.persist_state().await?;
        app.emit(
            "install-progress",
            progress(
                &variant_id,
                "complete",
                total,
                total,
                None,
                None,
                "complete",
            ),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub async fn start(
        &self,
        variant_id: String,
        target_id: String,
        password: Option<Zeroizing<String>>,
    ) -> Result<RuntimeStatus, String> {
        let catalog = self.catalog_clone().await?;
        let (model, variant) = find_variant(&catalog, &variant_id)?;
        let telemetry_model_id = model.id.clone();
        let telemetry_variant_id = variant.id.clone();
        ensure_supported_runtime(variant)?;
        let result: Result<RuntimeStatus, String> = async {
            if target_id != "local" && variant.runtime != OLLAMA_RUNTIME {
                return Err("Remote targets support Ollama catalog models only".to_owned());
            }
            if variant.runtime == DUMMY_RUNTIME {
                self.dummy_runtime
                    .start(&variant.runtime_ref)
                    .await
                    .map_err(|error| error.to_string())?;
            } else {
                let runtime = self
                    .runtime_for_target_with_password(&target_id, password)
                    .await?;
                if target_id == "local" {
                    runtime
                        .ensure_running()
                        .await
                        .map_err(|error| error.to_string())?;
                }
                if model_is_embedding_only(model) {
                    runtime
                        .start_embedding(&variant.runtime_ref)
                        .await
                        .map_err(|error| error.to_string())?;
                } else {
                    runtime
                        .start(&variant.runtime_ref)
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
            *self.installed_model.write().await = Some(variant.runtime_ref.clone());
            *self.selected_runtime.write().await = Some(variant.runtime.clone());
            self.persist_state().await?;
            Ok(RuntimeStatus {
                state: "running".to_owned(),
                model_id: Some(variant.runtime_ref.clone()),
                message: Some(if target_id == "local" {
                    "Listening on localhost".to_owned()
                } else {
                    "Listening through the remote SSH tunnel".to_owned()
                }),
            })
        }
        .await;
        self.telemetry.record(TelemetryEvent::ModelStart {
            model_id: telemetry_model_id,
            variant_id: telemetry_variant_id,
            deployment: deployment_kind(&target_id),
            succeeded: result.is_ok(),
            failure: result.as_ref().err().map(|error| failure_category(error)),
        });
        result
    }

    pub async fn stop(
        &self,
        variant_id: String,
        target_id: String,
        password: Option<Zeroizing<String>>,
    ) -> Result<RuntimeStatus, String> {
        let catalog = self.catalog_clone().await?;
        let (model, variant) = find_variant(&catalog, &variant_id)?;
        ensure_supported_runtime(variant)?;
        {
            let active = self.active_chat.lock().await;
            if active.as_ref().is_some_and(|chat| {
                same_ollama_reference(&chat.runtime_model_id, &variant.runtime_ref)
            }) {
                if let Some(chat) = active.as_ref() {
                    chat.cancellation.cancel();
                }
            }
        }
        if target_id != "local" && variant.runtime != OLLAMA_RUNTIME {
            return Err("Remote targets support Ollama catalog models only".to_owned());
        }
        if variant.runtime == DUMMY_RUNTIME {
            self.dummy_runtime
                .stop(&variant.runtime_ref)
                .await
                .map_err(|error| error.to_string())?;
        } else {
            let runtime = self
                .runtime_for_target_with_password(&target_id, password)
                .await?;
            if model_is_embedding_only(model) {
                runtime
                    .stop_embedding(&variant.runtime_ref)
                    .await
                    .map_err(|error| error.to_string())?;
            } else {
                runtime
                    .stop(&variant.runtime_ref)
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(RuntimeStatus::stopped())
    }

    pub async fn status(&self) -> Result<RuntimeStatus, String> {
        if let CoreRuntimeStatus::Running { models } = self
            .dummy_runtime
            .status()
            .await
            .map_err(|error| error.to_string())?
        {
            return Ok(RuntimeStatus {
                state: "running".to_owned(),
                model_id: models.first().cloned(),
                message: Some("Dummy test model is marked as running".to_owned()),
            });
        }
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

    pub async fn performance(
        &self,
        model_id: &str,
        runtime_model_id: &str,
        target_id: &str,
    ) -> Result<PerformanceSnapshot, String> {
        let catalog = self.catalog_clone().await?;
        let is_dummy = catalog.models.iter().any(|model| {
            model
                .variants
                .iter()
                .any(|variant| variant.id == model_id && variant.runtime == DUMMY_RUNTIME)
        });
        let runtime = if is_dummy {
            None
        } else {
            Some(self.runtime_for_target(target_id).await?)
        };
        let runtime_status = if is_dummy {
            self.dummy_runtime.status().await
        } else {
            runtime
                .as_ref()
                .ok_or_else(|| "The Ollama runtime is unavailable".to_owned())?
                .status()
                .await
        }
        .map_err(|error| error.to_string())?;
        let state = match runtime_status {
            CoreRuntimeStatus::Unavailable => "unavailable",
            CoreRuntimeStatus::Idle => "stopped",
            CoreRuntimeStatus::Running { models } => {
                let is_running = if is_dummy {
                    models.iter().any(|model| model == runtime_model_id)
                } else {
                    models
                        .iter()
                        .any(|model| same_ollama_reference(model, runtime_model_id))
                };
                if is_running {
                    "running"
                } else {
                    "stopped"
                }
            }
        };
        let allocation = if !is_dummy && state == "running" {
            runtime
                .as_ref()
                .ok_or_else(|| "The Ollama runtime is unavailable".to_owned())?
                .model_allocation(runtime_model_id)
                .await
                .map_err(|error| error.to_string())?
        } else {
            None
        };
        let allocated_memory_bytes = allocation
            .as_ref()
            .map(|metrics| metrics.total_memory_bytes)
            .unwrap_or_default();
        let allocated_vram_bytes = allocation
            .as_ref()
            .map(|metrics| metrics.vram_memory_bytes)
            .unwrap_or_default();
        Ok(PerformanceSnapshot {
            model_id: runtime_model_id.to_owned(),
            state: state.to_owned(),
            sampled_at_unix_ms: current_unix_time_ms(),
            allocated_memory_bytes,
            allocated_vram_bytes,
            allocated_system_memory_bytes: allocated_memory_bytes
                .saturating_sub(allocated_vram_bytes),
            context_length: allocation.and_then(|metrics| metrics.context_length),
        })
    }

    pub async fn chat(
        &self,
        model_id: &str,
        runtime_model_id: &str,
        target_id: &str,
        messages: Vec<ChatMessage>,
        reporter: &(dyn Fn(ChatEvent) + Send + Sync),
    ) -> Result<(), String> {
        validate_chat_messages(&messages)?;
        let catalog = self.catalog_clone().await?;
        let (model, variant) = find_variant(&catalog, model_id)?;
        if variant.runtime != OLLAMA_RUNTIME {
            return Err("This model does not expose a chat API".to_owned());
        }
        if !model_supports_chat(model) {
            return Err(if model_is_embedding_only(model) {
                "This embedding model does not support chat".to_owned()
            } else {
                "This model does not support chat".to_owned()
            });
        }
        if !same_ollama_reference(&variant.runtime_ref, runtime_model_id) {
            return Err("The runtime model identifier does not match the catalog entry".to_owned());
        }
        let runtime = self.runtime_for_target(target_id).await?;
        let running = runtime.status().await.map_err(|error| error.to_string())?;
        if !matches!(running, CoreRuntimeStatus::Running { ref models } if models.iter().any(|model| same_ollama_reference(model, runtime_model_id)))
        {
            return Err("Start this model before opening a chat".to_owned());
        }

        let cancellation = CancellationToken::new();
        {
            let mut active = self.active_chat.lock().await;
            if active.is_some() {
                return Err("Another chat response is already being generated".to_owned());
            }
            *active = Some(ActiveChat {
                runtime_model_id: runtime_model_id.to_owned(),
                cancellation: cancellation.clone(),
            });
        }

        let chat_reporter = |progress| match progress {
            ChatProgress::Content(content) => reporter(ChatEvent::Delta { content }),
            ChatProgress::Done => reporter(ChatEvent::Done),
        };
        let result = runtime
            .chat_cancellable(runtime_model_id, &messages, &chat_reporter, &cancellation)
            .await
            .map_err(|error| {
                if matches!(error, RuntimeError::Cancelled) {
                    "chat cancelled".to_owned()
                } else {
                    error.to_string()
                }
            });
        self.active_chat.lock().await.take();
        self.telemetry.record(TelemetryEvent::Chat {
            model_id: model.id.clone(),
            variant_id: variant.id.clone(),
            deployment: deployment_kind(target_id),
            outcome: match &result {
                Ok(()) => ChatOutcome::Succeeded,
                Err(error) if error == "chat cancelled" => ChatOutcome::Cancelled,
                Err(_) => ChatOutcome::Failed,
            },
        });
        result
    }

    pub async fn cancel_chat(&self) -> bool {
        let active = self.active_chat.lock().await;
        if let Some(chat) = active.as_ref() {
            chat.cancellation.cancel();
            true
        } else {
            false
        }
    }

    pub async fn endpoint(&self, target_id: &str) -> Result<EndpointDetails, String> {
        let model = self
            .installed_model
            .read()
            .await
            .clone()
            .ok_or_else(|| "Start a model before requesting endpoint details".to_owned())?;
        let is_dummy = self.selected_runtime.read().await.as_deref() == Some(DUMMY_RUNTIME);
        let catalog = self.catalog_clone().await?;
        let selected_runtime = self.selected_runtime.read().await.clone();
        let selected_model = catalog.models.iter().find(|entry| {
            entry.variants.iter().any(|variant| {
                selected_runtime.as_deref() == Some(variant.runtime.as_str())
                    && (variant.runtime_ref == model
                        || (variant.runtime == OLLAMA_RUNTIME
                            && same_ollama_reference(&variant.runtime_ref, &model)))
            })
        });
        let chat_available = selected_model.is_none_or(model_supports_chat);
        let embeddings_available = selected_model.is_some_and(model_is_embedding_only);
        let endpoint = if is_dummy {
            self.dummy_runtime.endpoint()
        } else {
            self.runtime_for_target(target_id).await?.endpoint()
        };
        endpoint_details(
            endpoint,
            model,
            !is_dummy,
            chat_available,
            embeddings_available,
        )
    }

    pub async fn model_endpoint(
        &self,
        model_id: &str,
        runtime_model_id: &str,
        target_id: &str,
    ) -> Result<EndpointDetails, String> {
        if runtime_model_id.trim().is_empty() {
            return Err("The model does not expose a runtime model identifier".to_owned());
        }
        let catalog = self.catalog_clone().await?;
        let (model, variant) = find_variant(&catalog, model_id)?;
        let is_dummy = variant.runtime == DUMMY_RUNTIME;
        let endpoint = if is_dummy {
            self.dummy_runtime.endpoint()
        } else {
            self.runtime_for_target(target_id).await?.endpoint()
        };
        endpoint_details(
            endpoint,
            runtime_model_id.to_owned(),
            !is_dummy,
            model_supports_chat(model),
            model_is_embedding_only(model),
        )
    }

    pub async fn load_models(&self) -> Result<Vec<PersistedModelEntry>, String> {
        let persisted = self.state.read().await.models.clone();
        let (local_persisted, mut remote_persisted): (Vec<_>, Vec<_>) = persisted
            .into_iter()
            .partition(|model| model.target_id == "local");
        for model in &mut remote_persisted {
            model.running = false;
        }
        let catalog = self.catalog_clone().await?;
        let dummy_installed = self
            .dummy_runtime
            .installed_models()
            .await
            .map_err(|error| error.to_string())?;
        let dummy_running = match self
            .dummy_runtime
            .status()
            .await
            .map_err(|error| error.to_string())?
        {
            CoreRuntimeStatus::Running { models } => models,
            CoreRuntimeStatus::Unavailable | CoreRuntimeStatus::Idle => Vec::new(),
        };
        if self.runtime.health().await.is_err() {
            let settings = self.state.read().await.settings.clone();
            if !settings.auto_start_managed_runtimes || !self.runtime.executable_available().await {
                let models = reconcile_unavailable_models(
                    &catalog,
                    local_persisted,
                    &dummy_installed,
                    &dummy_running,
                );
                let models = with_remote_models(models, remote_persisted);
                self.replace_models(models.clone()).await?;
                return Ok(models);
            }
            self.runtime
                .ensure_running()
                .await
                .map_err(|error| error.to_string())?;
        }

        let installed = self
            .host
            .installed_models()
            .await
            .map_err(|error| error.to_string())?;
        let mut running = match self
            .runtime
            .status()
            .await
            .map_err(|error| error.to_string())?
        {
            CoreRuntimeStatus::Running { models } => models,
            CoreRuntimeStatus::Unavailable | CoreRuntimeStatus::Idle => Vec::new(),
        };
        running.extend(dummy_running);
        if let Some(model) = running.first() {
            *self.installed_model.write().await = Some(model.clone());
        }
        let models = reconcile_models(
            catalog,
            local_persisted,
            installed,
            &dummy_installed,
            &running,
        );
        let models = with_remote_models(models, remote_persisted);
        self.replace_models(models.clone()).await?;
        Ok(models)
    }

    pub async fn save_models(&self, models: Vec<PersistedModelEntry>) -> Result<(), String> {
        self.replace_models(models).await
    }

    pub async fn remove_model(&self, model_id: &str) -> Result<Vec<PersistedModelEntry>, String> {
        let entry = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == model_id)
            .cloned()
            .ok_or_else(|| "The model is no longer in the local model list".to_owned())?;
        let target_id = entry.target_id.clone();
        let runtime_model_id = entry.runtime_model_id.as_deref().ok_or_else(|| {
            "The model has no runtime identifier and cannot be deleted".to_owned()
        })?;
        let catalog = self.catalog_clone().await?;
        let telemetry_model_id = catalog
            .models
            .iter()
            .find(|model| {
                model
                    .variants
                    .iter()
                    .any(|variant| variant.id == entry.model_id)
            })
            .map(|model| model.id.clone())
            .unwrap_or_else(|| "custom".to_owned());
        let telemetry_variant_id = if entry.managed {
            entry.model_id.clone()
        } else {
            "custom".to_owned()
        };
        let is_dummy = catalog.models.iter().any(|model| {
            model
                .variants
                .iter()
                .any(|variant| variant.id == entry.model_id && variant.runtime == DUMMY_RUNTIME)
        });

        let result: Result<Vec<PersistedModelEntry>, String> = async {
        if is_dummy {
            let status = self
                .dummy_runtime
                .status()
                .await
                .map_err(|error| error.to_string())?;
            if matches!(status, CoreRuntimeStatus::Running { ref models } if models.iter().any(|model| model == runtime_model_id))
            {
                self.dummy_runtime
                    .stop(runtime_model_id)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            self.dummy_runtime
                .delete_model(runtime_model_id)
                .await
                .map_err(|error| error.to_string())?;
        } else {
            let runtime = self.runtime_for_target(&target_id).await?;
            if target_id == "local" {
                runtime
                    .ensure_running()
                    .await
                    .map_err(|error| error.to_string())?;
            }
            let status = runtime.status().await.map_err(|error| error.to_string())?;
            if matches!(status, CoreRuntimeStatus::Running { ref models } if models.iter().any(|model| same_ollama_reference(model, runtime_model_id)))
            {
                runtime
                    .stop(runtime_model_id)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            runtime
                .delete_model(runtime_model_id)
                .await
                .map_err(|error| error.to_string())?;
        }

        let models = {
            let mut state = self.state.write().await;
            state.models.retain(|model| {
                model.target_id != target_id
                    || !model.runtime_model_id.as_deref().is_some_and(|runtime_id| {
                        same_ollama_reference(runtime_id, runtime_model_id)
                    })
            });
            state.models.clone()
        };
        if self
            .installed_model
            .read()
            .await
            .as_deref()
            .is_some_and(|installed| same_ollama_reference(installed, runtime_model_id))
        {
            *self.installed_model.write().await = None;
            *self.selected_runtime.write().await = None;
        }
        self.persist_state().await?;
        Ok(models)
        }
        .await;
        self.telemetry.record(TelemetryEvent::ModelUninstall {
            model_id: telemetry_model_id,
            variant_id: telemetry_variant_id,
            deployment: deployment_kind(&target_id),
            succeeded: result.is_ok(),
            failure: result.as_ref().err().map(|error| failure_category(error)),
        });
        result
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
        let installed_model = self.installed_model.read().await.clone();
        let selected_runtime = self.selected_runtime.read().await.clone();
        let runtime_executable = Some(self.runtime.executable_path().await);
        {
            let mut state = self.state.write().await;
            state.installed_model = installed_model;
            state.selected_runtime = selected_runtime;
            state.runtime_executable = runtime_executable;
        }
        self.flush_state().await
    }

    async fn replace_models(&self, models: Vec<PersistedModelEntry>) -> Result<(), String> {
        let retention = self
            .state
            .read()
            .await
            .settings
            .privacy
            .lifecycle_log_retention as usize;
        let models = models
            .into_iter()
            .map(|mut model| {
                if model.logs.len() > retention {
                    model.logs.drain(..model.logs.len() - retention);
                }
                model
            })
            .collect();
        self.state.write().await.models = models;
        self.flush_state().await
    }

    async fn flush_state(&self) -> Result<(), String> {
        let _write = self.state_write.lock().await;
        let state = self.state.read().await.clone();
        let bytes = serde_json::to_vec_pretty(&state).map_err(|error| error.to_string())?;
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

fn default_ollama_executable() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let installed = PathBuf::from(local_app_data)
                .join("Programs")
                .join("Ollama")
                .join("ollama.exe");
            if installed.is_file() {
                return installed;
            }
        }
        PathBuf::from("ollama.exe")
    }
    #[cfg(not(target_os = "windows"))]
    PathBuf::from("ollama")
}

fn resolve_ollama_executable(persisted: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if persisted == std::path::Path::new("ollama")
            || persisted == std::path::Path::new("ollama.exe")
            || !persisted.is_file()
        {
            return default_ollama_executable();
        }
    }
    persisted.to_path_buf()
}

fn bundled_catalog() -> Result<LoadedCatalog, String> {
    Ok(LoadedCatalog {
        catalog: Catalog::from_slice(BUNDLED_CATALOG).map_err(|error| error.to_string())?,
        source: "bundled".to_owned(),
    })
}

fn add_development_catalog_entries(catalog: &mut Catalog) {
    #[cfg(debug_assertions)]
    {
        let Ok(development) = Catalog::from_slice(DEVELOPMENT_CATALOG) else {
            return;
        };
        for runtime in development
            .runtimes
            .into_iter()
            .filter(|runtime| runtime.id == DUMMY_RUNTIME)
        {
            if !catalog.runtimes.iter().any(|entry| entry.id == runtime.id) {
                catalog.runtimes.push(runtime);
            }
        }
        for model in development
            .models
            .into_iter()
            .filter(|model| model.id == "dummy-test-model")
        {
            if !catalog.models.iter().any(|entry| entry.id == model.id) {
                catalog.models.push(model);
            }
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = catalog;
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
                    loaded
                        .catalog
                        .runtimes
                        .iter()
                        .find(|runtime| runtime.id == variant.runtime)
                        .map(|runtime| CatalogModelSummary {
                            id: model.id.clone(),
                            display_name: model.display_name.clone(),
                            version: runtime.install.version.clone(),
                            description: model.description.clone(),
                        })
                })
            })
            .collect(),
    }
}

fn intent_use_case(intent: &str) -> Result<&'static str, String> {
    match intent {
        "chat" => Ok("general"),
        "code" => Ok("programming"),
        "creative" => Ok("general"),
        "research" => Ok("rag"),
        _ => Err("Choose a supported use before requesting recommendations".to_owned()),
    }
}

fn validate_chat_messages(messages: &[ChatMessage]) -> Result<(), String> {
    const MAX_MESSAGES: usize = 100;
    const MAX_MESSAGE_BYTES: usize = 64 * 1024;
    const MAX_CONVERSATION_BYTES: usize = 512 * 1024;

    if messages.is_empty() {
        return Err("Enter a message before starting a chat".to_owned());
    }
    if messages.len() > MAX_MESSAGES {
        return Err("Clear the chat before sending more than 100 messages".to_owned());
    }
    let mut conversation_bytes = 0usize;
    for message in messages {
        if !matches!(message.role.as_str(), "user" | "assistant") {
            return Err("Chat messages must use the user or assistant role".to_owned());
        }
        if message.content.trim().is_empty() {
            return Err("Chat messages cannot be empty".to_owned());
        }
        if message.content.len() > MAX_MESSAGE_BYTES {
            return Err("A chat message cannot exceed 64 KiB".to_owned());
        }
        conversation_bytes = conversation_bytes.saturating_add(message.content.len());
    }
    if conversation_bytes > MAX_CONVERSATION_BYTES {
        return Err("Clear the chat before the conversation exceeds 512 KiB".to_owned());
    }
    Ok(())
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

fn ensure_supported_runtime(variant: &ModelVariant) -> Result<(), String> {
    if matches!(variant.runtime.as_str(), OLLAMA_RUNTIME | DUMMY_RUNTIME) {
        Ok(())
    } else {
        Err(format!(
            "Runtime `{}` is not supported by this version of Lumen Source",
            variant.runtime
        ))
    }
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
    if !executable_name.ends_with(".tar.zst") && !executable_name.ends_with(".zip") {
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

fn check(id: &str, status: &str, message_key: &str, detail: Option<&str>) -> PreflightCheck {
    PreflightCheck {
        id: id.to_owned(),
        status: status.to_owned(),
        message_key: message_key.to_owned(),
        detail: detail.map(str::to_owned),
    }
}

fn progress(
    model_id: &str,
    phase: &str,
    completed: u64,
    total: u64,
    current_item: Option<u32>,
    total_items: Option<u32>,
    message_key: &str,
) -> InstallProgress {
    InstallProgress {
        model_id: model_id.to_owned(),
        phase: phase.to_owned(),
        completed_bytes: completed,
        total_bytes: total,
        current_item,
        total_items,
        message_key: message_key.to_owned(),
        detail: None,
    }
}

fn runtime_progress(
    model_id: &str,
    fallback_total: u64,
    expected_items: Option<u32>,
    pull_items: &StdMutex<PullItemTracker>,
    event: RuntimeProgress,
) -> InstallProgress {
    match event {
        RuntimeProgress::Downloading { downloaded, total } => progress(
            model_id,
            "downloading",
            downloaded,
            total.unwrap_or(fallback_total),
            None,
            None,
            "downloadRuntime",
        ),
        RuntimeProgress::Verifying => progress(
            model_id,
            "verifying",
            fallback_total,
            fallback_total,
            None,
            None,
            "verifyDownload",
        ),
        RuntimeProgress::Installing => progress(
            model_id,
            "installing",
            fallback_total,
            fallback_total,
            None,
            None,
            "installRuntime",
        ),
        RuntimeProgress::PullingModel {
            status,
            digest,
            completed,
            total,
        } => {
            let current_item = digest.as_deref().and_then(|digest| {
                pull_items
                    .lock()
                    .ok()
                    .map(|mut tracker| tracker.observe(digest))
            });
            let total_items =
                current_item.map(|current| expected_items.unwrap_or(current).max(current));
            let mut payload = progress(
                model_id,
                "downloading",
                completed.unwrap_or_default(),
                total.unwrap_or(fallback_total),
                current_item,
                total_items,
                "pullModel",
            );
            payload.detail = Some(status);
            payload
        }
        RuntimeProgress::Ready => progress(
            model_id,
            "installing",
            fallback_total,
            fallback_total,
            None,
            None,
            "registerModel",
        ),
    }
}

fn endpoint_details(
    endpoint: RuntimeEndpoint,
    model: String,
    api_available: bool,
    chat_available: bool,
    embeddings_available: bool,
) -> Result<EndpointDetails, String> {
    let base = endpoint.base_url.as_str().trim_end_matches('/');
    if base.is_empty() {
        return Err("Runtime endpoint is empty".to_owned());
    }
    Ok(EndpointDetails {
        base_url: format!("{base}/v1"),
        chat_completions_url: format!("{base}/v1/chat/completions"),
        completions_url: format!("{base}/v1/completions"),
        embeddings_url: format!("{base}/v1/embeddings"),
        model,
        api_key_required: false,
        api_available,
        chat_available,
        embeddings_available,
    })
}

fn model_is_embedding_only(model: &ModelEntry) -> bool {
    model
        .capabilities
        .iter()
        .any(|capability| capability == "embeddings")
        && !model_supports_chat(model)
}

fn model_supports_chat(model: &ModelEntry) -> bool {
    model
        .capabilities
        .iter()
        .any(|capability| capability == "chat" || capability == "text-generation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_source_runtime::InstalledModel;

    #[test]
    fn production_catalog_defaults_use_https_and_a_32_byte_ed25519_key() {
        assert!(PRODUCTION_CATALOG_URL.starts_with("https://"));
        assert!(PRODUCTION_CATALOG_SIGNATURE_URL.starts_with("https://"));
        let Ok(public_key) = STANDARD.decode(PRODUCTION_CATALOG_PUBLIC_KEY) else {
            panic!("production catalog public key should be valid base64");
        };

        assert_eq!(public_key.len(), 32);
    }

    #[test]
    fn v04_state_without_settings_keeps_models_and_uses_ollama_defaults() {
        let Ok(state): Result<PersistedState, _> = serde_json::from_str(
            r#"{
                "installed_model": "qwen3:8b",
                "selected_runtime": "ollama",
                "runtime_executable": "ollama.exe",
                "remote_targets": [],
                "models": [{
                    "id": "legacy-qwen",
                    "name": "My Qwen",
                    "modelId": "qwen3-8b-q4_k_m",
                    "modelName": "Qwen 3",
                    "version": "3",
                    "location": "local",
                    "running": false,
                    "logs": ["Installed before settings existed."]
                }]
            }"#,
        ) else {
            panic!("v0.4 state should deserialize");
        };

        assert_eq!(state.installed_model.as_deref(), Some("qwen3:8b"));
        assert_eq!(
            state.settings.default_runtime,
            crate::settings::RuntimeId::Ollama
        );
        assert_eq!(state.settings.ollama.endpoint, "http://127.0.0.1:11434");
        assert_eq!(state.models.len(), 1);
        assert_eq!(state.models[0].name, "My Qwen");
        assert_eq!(state.models[0].logs, ["Installed before settings existed."]);
    }

    #[test]
    fn maps_every_wizard_intent_to_a_catalog_use_case() {
        assert_eq!(intent_use_case("chat"), Ok("general"));
        assert_eq!(intent_use_case("code"), Ok("programming"));
        assert_eq!(intent_use_case("creative"), Ok("general"));
        assert_eq!(intent_use_case("research"), Ok("rag"));
        assert!(intent_use_case("unsupported").is_err());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_catalog_adds_the_dummy_model_to_the_generated_model_list() {
        let Ok(mut catalog) = Catalog::from_slice(BUNDLED_CATALOG) else {
            panic!("bundled model list should parse");
        };

        add_development_catalog_entries(&mut catalog);

        assert!(catalog
            .models
            .iter()
            .any(|model| model.id == "dummy-test-model"));
        assert!(catalog
            .runtimes
            .iter()
            .any(|runtime| runtime.id == DUMMY_RUNTIME));
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn release_catalog_does_not_add_the_dummy_model() {
        let Ok(mut catalog) = Catalog::from_slice(BUNDLED_CATALOG) else {
            panic!("bundled model list should parse");
        };

        add_development_catalog_entries(&mut catalog);

        assert!(!catalog
            .models
            .iter()
            .any(|model| model.id == "dummy-test-model"));
        assert!(!catalog
            .runtimes
            .iter()
            .any(|runtime| runtime.id == DUMMY_RUNTIME));
    }

    #[test]
    fn builds_openai_compatible_endpoint_details() {
        let Ok(base_url) = Url::parse("http://127.0.0.1:11434") else {
            panic!("fixed test URL should be valid");
        };
        let Ok(details) = endpoint_details(
            RuntimeEndpoint { base_url },
            "qwen2.5-coder:14b".to_owned(),
            true,
            true,
            false,
        ) else {
            panic!("fixed endpoint should produce connection details");
        };

        assert_eq!(details.base_url, "http://127.0.0.1:11434/v1");
        assert_eq!(
            details.chat_completions_url,
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(details.model, "qwen2.5-coder:14b");
        assert_eq!(
            details.embeddings_url,
            "http://127.0.0.1:11434/v1/embeddings"
        );
        assert_eq!(
            details.completions_url,
            "http://127.0.0.1:11434/v1/completions"
        );
        assert!(!details.api_key_required);
        assert!(details.api_available);
        assert!(details.chat_available);
        assert!(!details.embeddings_available);
    }

    #[test]
    fn identifies_embedding_only_catalog_models() {
        let Ok(catalog) = Catalog::from_slice(BUNDLED_CATALOG) else {
            panic!("bundled model list should parse");
        };
        let Some(model) = catalog
            .models
            .iter()
            .find(|model| model.id == "baai-bge-m3")
        else {
            panic!("BGE-M3 should remain in the bundled catalog");
        };

        assert!(model_is_embedding_only(model));
        assert!(!model_supports_chat(model));
    }

    #[test]
    fn reconciles_catalog_and_external_ollama_models() {
        let Ok(catalog) = Catalog::from_slice(TEST_CATALOG) else {
            panic!("bundled test catalog should parse");
        };
        let installed = vec![
            InstalledModel {
                name: "qwen2.5-coder:14b".to_owned(),
                digest: Some("sha256:qwen".to_owned()),
                size_bytes: Some(9_000),
            },
            InstalledModel {
                name: "outside-model".to_owned(),
                digest: Some("sha256:outside".to_owned()),
                size_bytes: Some(500),
            },
        ];

        let models = reconcile_models(
            catalog,
            Vec::new(),
            installed,
            &[],
            &["qwen2.5-coder:14b".to_owned()],
        );

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].model_id, "qwen2.5-coder-14b-q4_k_m");
        assert!(models[0].managed);
        assert!(models[0].running);
        let Some(external) = models.iter().find(|model| !model.managed) else {
            panic!("external Ollama model should remain visible");
        };
        assert_eq!(external.runtime_model_id.as_deref(), Some("outside-model"));
        assert!(!external.running);
    }

    #[test]
    fn reconciliation_preserves_multiple_entries_for_the_same_ollama_model() {
        let Ok(catalog) = Catalog::from_slice(TEST_CATALOG) else {
            panic!("bundled test catalog should parse");
        };
        let entry = |id: &str, name: &str| PersistedModelEntry {
            id: id.to_owned(),
            name: name.to_owned(),
            model_id: "qwen2.5-coder-14b-q4_k_m".to_owned(),
            model_name: "Qwen2.5 Coder 14B".to_owned(),
            runtime_model_id: Some("qwen2.5-coder:14b".to_owned()),
            version: "0.32.1".to_owned(),
            location: "local".to_owned(),
            target_id: local_target_id(),
            target_name: None,
            running: false,
            managed: true,
            digest: None,
            size_bytes: None,
            license_basis: None,
            license_reference: None,
            license_acknowledged_at: None,
            license_profile_id: None,
            license_name: None,
            license_url: None,
            license_reviewed_at: None,
            license_catalog_version: None,
            logs: Vec::new(),
        };

        let models = reconcile_models(
            catalog,
            vec![
                entry("first-row", "First copy"),
                entry("second-row", "Second copy"),
            ],
            vec![InstalledModel {
                name: "qwen2.5-coder:14b".to_owned(),
                digest: Some("sha256:qwen".to_owned()),
                size_bytes: Some(9_000),
            }],
            &[],
            &["qwen2.5-coder:14b".to_owned()],
        );

        assert_eq!(models.len(), 2);
        assert!(models.iter().any(|model| model.id == "first-row"));
        assert!(models.iter().any(|model| model.id == "second-row"));
        assert!(models.iter().all(|model| model.running));
    }

    #[test]
    fn preserves_the_persisted_dummy_model_for_ui_testing() {
        let Ok(catalog) = Catalog::from_slice(TEST_CATALOG) else {
            panic!("bundled test catalog should parse");
        };
        let persisted = PersistedModelEntry {
            id: "dummy-row".to_owned(),
            name: "Dummy Test Model".to_owned(),
            model_id: "dummy-test-model-variant".to_owned(),
            model_name: "Dummy Test Model".to_owned(),
            runtime_model_id: Some("dummy-test-model:latest".to_owned()),
            version: "0.0.0-dummy".to_owned(),
            location: "local".to_owned(),
            target_id: local_target_id(),
            target_name: None,
            running: false,
            managed: true,
            digest: None,
            size_bytes: Some(0),
            license_basis: None,
            license_reference: None,
            license_acknowledged_at: None,
            license_profile_id: None,
            license_name: None,
            license_url: None,
            license_reviewed_at: None,
            license_catalog_version: None,
            logs: vec!["Installed for a UI test.".to_owned()],
        };

        let models = reconcile_models(
            catalog,
            vec![persisted],
            Vec::new(),
            &[],
            &["dummy-test-model:latest".to_owned()],
        );

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "dummy-test-model-variant");
        assert!(models[0].running);
    }

    #[test]
    fn unavailable_ollama_preserves_multiple_models_and_live_dummy_state() {
        let Ok(catalog) = Catalog::from_slice(TEST_CATALOG) else {
            panic!("bundled test catalog should parse");
        };
        let dummy = PersistedModelEntry {
            id: "dummy-row".to_owned(),
            name: "Dummy Test Model".to_owned(),
            model_id: "dummy-test-model-variant".to_owned(),
            model_name: "Dummy Test Model".to_owned(),
            runtime_model_id: Some("dummy-test-model:latest".to_owned()),
            version: "0.0.0-dummy".to_owned(),
            location: "local".to_owned(),
            target_id: local_target_id(),
            target_name: None,
            running: false,
            managed: true,
            digest: None,
            size_bytes: Some(0),
            license_basis: None,
            license_reference: None,
            license_acknowledged_at: None,
            license_profile_id: None,
            license_name: None,
            license_url: None,
            license_reviewed_at: None,
            license_catalog_version: None,
            logs: Vec::new(),
        };
        let ollama = PersistedModelEntry {
            id: "qwen-row".to_owned(),
            name: "Qwen".to_owned(),
            model_id: "qwen2.5-coder-14b-q4_k_m".to_owned(),
            model_name: "Qwen2.5 Coder 14B".to_owned(),
            runtime_model_id: Some("qwen2.5-coder:14b".to_owned()),
            version: "0.32.1".to_owned(),
            location: "local".to_owned(),
            target_id: local_target_id(),
            target_name: None,
            running: true,
            managed: true,
            digest: None,
            size_bytes: None,
            license_basis: None,
            license_reference: None,
            license_acknowledged_at: None,
            license_profile_id: None,
            license_name: None,
            license_url: None,
            license_reviewed_at: None,
            license_catalog_version: None,
            logs: Vec::new(),
        };

        let models = reconcile_unavailable_models(
            &catalog,
            vec![ollama, dummy],
            &[],
            &["dummy-test-model:latest".to_owned()],
        );

        assert_eq!(models.len(), 2);
        let Some(dummy) = models
            .iter()
            .find(|model| model.model_id == "dummy-test-model-variant")
        else {
            panic!("dummy model should be retained");
        };
        assert!(dummy.running);
        let Some(ollama) = models
            .iter()
            .find(|model| model.model_id == "qwen2.5-coder-14b-q4_k_m")
        else {
            panic!("Ollama model should be retained while its runtime is unavailable");
        };
        assert!(!ollama.running);
    }

    #[test]
    fn reconciliation_keeps_ollama_and_dummy_models_together() {
        let Ok(catalog) = Catalog::from_slice(TEST_CATALOG) else {
            panic!("bundled test catalog should parse");
        };
        let installed = vec![InstalledModel {
            name: "qwen2.5-coder:14b".to_owned(),
            digest: Some("sha256:qwen".to_owned()),
            size_bytes: Some(9_000),
        }];

        let models = reconcile_models(
            catalog,
            Vec::new(),
            installed,
            &[InstalledModel {
                name: "dummy-test-model:latest".to_owned(),
                digest: Some("dummy:dummy-test-model:latest".to_owned()),
                size_bytes: Some(0),
            }],
            &["dummy-test-model:latest".to_owned()],
        );

        assert_eq!(models.len(), 2);
        assert!(models
            .iter()
            .any(|model| model.model_id == "dummy-test-model-variant"));
        assert!(models
            .iter()
            .any(|model| model.model_id == "qwen2.5-coder-14b-q4_k_m"));
    }

    #[test]
    fn treats_an_omitted_ollama_tag_as_latest() {
        assert!(same_ollama_reference("model", "model:latest"));
        assert!(same_ollama_reference(
            "localhost:5000/team/model",
            "localhost:5000/team/model:latest"
        ));
        assert!(!same_ollama_reference("model:small", "model:large"));
    }
}
