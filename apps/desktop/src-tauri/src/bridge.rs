//! Thin adaptation seam between Tauri and the shared Lumen Source crates.

use std::collections::BTreeSet;
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
    Artifact as RuntimeArtifact, ArtifactInstaller, CancellationToken, ChatMessage, ChatOptions,
    ChatProgress, DummyRuntime, OllamaRuntime, Runtime, RuntimeEndpoint, RuntimeError,
    RuntimeProgress, RuntimeStatus as CoreRuntimeStatus, Url, VllmRuntime,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use zeroize::Zeroizing;

pub use crate::bridge_types::*;
use crate::credential_store;
use crate::managed_vllm::{self, ManagedVllmSpec, ManagedVllmSupport};
use crate::model_reconciliation::{
    reconcile_models, reconcile_unavailable_models, same_ollama_reference, with_remote_models,
};
use crate::remote::{
    connect as connect_remote, probe_hardware as probe_remote_hardware,
    probe_usage as probe_remote_usage, RemoteAuthentication, RemoteConnectionReport, RemoteSession,
    RemoteTargetConfig, RemoteTargetProfile,
};
use crate::runtime_registry::{
    RuntimeId, RuntimeRegistry, DUMMY_RUNTIME, OLLAMA_RUNTIME, VLLM_RUNTIME,
};
use crate::settings::{
    migrate_settings, validate_external_vllm_config, validate_model_settings, validate_settings,
    ApplicationSettings, ExternalVllmConfig, ModelInferenceTask, ModelSettings,
    ModelSettingsSaveReport, OllamaConnectionReport, PerformanceProfile, RuntimeSecretKind,
    SettingsSaveReport, VllmConnectionReport,
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

#[derive(Clone)]
struct RecentInstall {
    variant_id: String,
    target_id: String,
    was_already_installed: bool,
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
    runtime_registry: RuntimeRegistry,
    host: LocalHost<PlatformHardwareProbe, OllamaRuntime>,
    catalog: RwLock<Option<LoadedCatalog>>,
    installed_model: RwLock<Option<String>>,
    selected_runtime: RwLock<Option<String>>,
    state: RwLock<PersistedState>,
    state_write: Mutex<()>,
    active_install: Mutex<Option<ActiveInstall>>,
    active_chat: Mutex<Option<ActiveChat>>,
    recent_install: RwLock<Option<RecentInstall>>,
    managed_ports: Mutex<BTreeSet<u16>>,
    remote_session: RwLock<Option<Arc<RemoteSession>>>,
    state_path: PathBuf,
    telemetry: Telemetry,
}

pub struct InstallOptions {
    pub performance_profile: PerformanceProfile,
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
        let managed_ports = persisted
            .models
            .iter()
            .filter_map(|model| model.model_settings.as_ref()?.managed_port)
            .collect();
        Ok(Self {
            probe,
            runtime,
            dummy_runtime,
            runtime_registry: RuntimeRegistry::default(),
            host,
            catalog: RwLock::new(None),
            installed_model: RwLock::new(persisted.installed_model.clone()),
            selected_runtime: RwLock::new(persisted.selected_runtime.clone()),
            state: RwLock::new(persisted),
            state_write: Mutex::new(()),
            active_install: Mutex::new(None),
            active_chat: Mutex::new(None),
            recent_install: RwLock::new(None),
            managed_ports: Mutex::new(managed_ports),
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

    pub async fn test_vllm_connection(
        &self,
        config: ExternalVllmConfig,
        api_key: Option<Zeroizing<String>>,
        entry_id: Option<&str>,
    ) -> VllmConnectionReport {
        let endpoint = config.endpoint.clone();
        let validation = validate_external_vllm_config(&config, false);
        if !validation.is_empty() {
            return VllmConnectionReport {
                healthy: false,
                authenticated: false,
                endpoint,
                models: Vec::new(),
                message: validation
                    .into_iter()
                    .map(|error| error.message)
                    .collect::<Vec<_>>()
                    .join(" "),
            };
        }
        let stored_key = if api_key.is_none() {
            match entry_id {
                Some(entry_id) => credential_store::load_runtime_secret_for_account(
                    RuntimeSecretKind::VllmApiKey,
                    entry_id.to_owned(),
                )
                .await
                .ok()
                .flatten(),
                None => None,
            }
        } else {
            None
        };
        let key = api_key
            .as_deref()
            .map(String::as_str)
            .or_else(|| stored_key.as_deref().map(String::as_str));
        let runtime = match vllm_runtime(&config) {
            Ok(runtime) => runtime,
            Err(error) => {
                return VllmConnectionReport {
                    healthy: false,
                    authenticated: false,
                    endpoint,
                    models: Vec::new(),
                    message: error.to_string(),
                };
            }
        };
        match runtime.models(key).await {
            Ok(models) => VllmConnectionReport {
                healthy: true,
                authenticated: true,
                endpoint,
                message: if models.is_empty() {
                    "Connected, but this vLLM server is not currently serving a model.".to_owned()
                } else {
                    format!("Connected to vLLM. {} served model(s) found.", models.len())
                },
                models,
            },
            Err(RuntimeError::AuthenticationRejected) => VllmConnectionReport {
                healthy: false,
                authenticated: false,
                endpoint,
                models: Vec::new(),
                message: "vLLM rejected the API key. Enter the server's current key and retry."
                    .to_owned(),
            },
            Err(error) => VllmConnectionReport {
                healthy: false,
                authenticated: key.is_some(),
                endpoint,
                models: Vec::new(),
                message: format!("Could not connect to vLLM: {error}"),
            },
        }
    }

    pub async fn save_vllm_model(
        &self,
        entry_id: Option<String>,
        display_name: String,
        config: ExternalVllmConfig,
        api_key: Option<Zeroizing<String>>,
        clear_api_key: bool,
    ) -> Result<PersistedModelEntry, String> {
        let errors = validate_external_vllm_config(&config, true);
        if !errors.is_empty() {
            return Err(errors
                .into_iter()
                .map(|error| format!("{}: {}", error.field, error.message))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        let id = entry_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        // Clearing a credential must test the exact post-save state. Passing an
        // explicit empty value prevents the connection test from falling back
        // to the credential that is about to be removed.
        let test_api_key = if clear_api_key {
            Some(Zeroizing::new(String::new()))
        } else {
            api_key.clone()
        };
        let report = self
            .test_vllm_connection(config.clone(), test_api_key, Some(&id))
            .await;
        if !report.healthy {
            return Err(report.message);
        }
        if !report
            .models
            .iter()
            .any(|model| model == &config.served_model)
        {
            return Err(
                "The selected model is no longer reported by this vLLM endpoint. Test the connection again."
                    .to_owned(),
            );
        }
        let stored_validation_key = if api_key.is_none() && !clear_api_key {
            credential_store::load_runtime_secret_for_account(
                RuntimeSecretKind::VllmApiKey,
                id.clone(),
            )
            .await?
        } else {
            None
        };
        let validation_key = if clear_api_key {
            None
        } else {
            api_key
                .as_deref()
                .map(String::as_str)
                .or_else(|| stored_validation_key.as_deref().map(String::as_str))
        };
        let runtime = vllm_runtime(&config).map_err(|error| error.to_string())?;
        let settings = config.model_settings();
        let mut checks = vec![
            validation_check("runtime", "pass", "The external vLLM API is healthy."),
            validation_check(
                "identity",
                "pass",
                "vLLM reported the configured served-model identity.",
            ),
        ];
        if config.inference_task == ModelInferenceTask::Embeddings {
            let embedding = runtime
                .embeddings(
                    &config.served_model,
                    "LumenSource validation",
                    validation_key,
                )
                .await
                .map_err(|error| format!("The vLLM embedding validation failed: {error}"))?;
            if embedding.is_empty() {
                return Err("vLLM returned an empty embedding vector.".to_owned());
            }
            checks.push(validation_check(
                "inference",
                "pass",
                &format!(
                    "vLLM returned a non-empty {}-dimension embedding.",
                    embedding.len()
                ),
            ));
        } else {
            let response_bytes = StdMutex::new(0_usize);
            let reporter = |progress| {
                if let ChatProgress::Content(content) = progress {
                    if let Ok(mut total) = response_bytes.lock() {
                        *total = total.saturating_add(content.trim().len());
                    }
                }
            };
            let cancellation = CancellationToken::new();
            let mut options = chat_options(&settings);
            options.temperature = Some(0.0);
            options.max_output_tokens = Some(8);
            runtime
                .chat_with_options(
                    &config.served_model,
                    &[ChatMessage {
                        role: "user".to_owned(),
                        content: "Reply with OK.".to_owned(),
                    }],
                    validation_key,
                    &options,
                    &reporter,
                    &cancellation,
                )
                .await
                .map_err(|error| format!("The vLLM chat validation failed: {error}"))?;
            if response_bytes
                .lock()
                .map(|total| *total)
                .unwrap_or_default()
                == 0
            {
                return Err("vLLM returned an empty validation response.".to_owned());
            }
            checks.push(validation_check(
                "inference",
                "pass",
                "vLLM returned a non-empty deterministic validation response.",
            ));
        }
        checks.push(validation_check(
            "configuration",
            "warning",
            "This external service controls context and accelerator settings outside Lumen Source.",
        ));
        let validation = InstallationValidationReport {
            passed: true,
            capability: format!("{:?}", config.inference_task).to_ascii_lowercase(),
            runtime_id: VLLM_RUNTIME.to_owned(),
            runtime_model_id: config.served_model.clone(),
            message: "The external vLLM model produced a valid response and is ready.".to_owned(),
            accelerator: None,
            hardware_summary: None,
            effective_context_length: None,
            validated_at: chrono::Utc::now().to_rfc3339(),
            running: true,
            settings: settings.clone(),
            checks,
        };
        if clear_api_key {
            credential_store::delete_runtime_secret_for_account(
                RuntimeSecretKind::VllmApiKey,
                id.clone(),
            )
            .await?;
        } else if let Some(api_key) = api_key {
            credential_store::save_runtime_secret_for_account(
                RuntimeSecretKind::VllmApiKey,
                id.clone(),
                api_key,
            )
            .await?;
        }
        let existing = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == id)
            .cloned();
        let mut entry = PersistedModelEntry {
            id: id.clone(),
            name: if display_name.trim().is_empty() {
                config.served_model.clone()
            } else {
                display_name.trim().to_owned()
            },
            model_id: format!("external-vllm-{id}"),
            model_name: config.served_model.clone(),
            runtime_id: VLLM_RUNTIME.to_owned(),
            runtime_model_id: Some(config.served_model.clone()),
            runtime_capabilities: external_vllm_capabilities(
                &self.runtime_registry,
                config.inference_task,
            ),
            model_settings: Some(settings),
            installation_validation: Some(validation),
            version: "External service".to_owned(),
            location: "local".to_owned(),
            target_id: "local".to_owned(),
            target_name: None,
            running: true,
            managed: false,
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
            logs: existing.map_or_else(
                || vec!["Connected to an externally managed vLLM model.".to_owned()],
                |model| model.logs,
            ),
        };
        let retention = self
            .state
            .read()
            .await
            .settings
            .privacy
            .lifecycle_log_retention as usize;
        if entry.logs.len() > retention {
            entry.logs.drain(..entry.logs.len() - retention);
        }
        {
            let mut state = self.state.write().await;
            state.models.retain(|model| model.id != id);
            state.models.push(entry.clone());
        }
        self.flush_state().await?;
        Ok(entry)
    }

    pub async fn vllm_credential_status(&self, entry_id: &str) -> Result<bool, String> {
        credential_store::runtime_secret_is_saved_for_account(
            RuntimeSecretKind::VllmApiKey,
            entry_id.to_owned(),
        )
        .await
    }

    pub async fn save_model_settings(
        &self,
        entry_id: &str,
        settings: ModelSettings,
        apply_restart: bool,
    ) -> Result<ModelSettingsSaveReport, String> {
        let existing = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == entry_id)
            .cloned()
            .ok_or_else(|| "The model is no longer installed.".to_owned())?;
        let maximum_context = self
            .catalog_clone()
            .await?
            .models
            .iter()
            .flat_map(|model| &model.variants)
            .find(|variant| variant.id == existing.model_id)
            .and_then(|variant| variant.context_window_tokens);
        let errors = validate_model_settings(&settings, maximum_context);
        if !errors.is_empty() {
            return Err(errors
                .into_iter()
                .map(|error| format!("{}: {}", error.field, error.message))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        let previous = existing.model_settings.clone().unwrap_or_default();
        if existing.runtime_capabilities.lifecycle
            == Some(crate::runtime_registry::RuntimeLifecycle::External)
            && external_engine_settings_changed(&previous, &settings)
        {
            return Err(
                "Engine and load settings are read-only for an externally managed service."
                    .to_owned(),
            );
        }
        let restart_required = load_settings_changed(&previous, &settings, &existing.runtime_id);
        let mut updated = existing.clone();
        updated.model_settings = Some(settings);
        updated.logs.push(format!(
            "[{}] Model settings changed{}.",
            current_timestamp(),
            if restart_required {
                "; a restart is required"
            } else {
                ""
            }
        ));
        {
            let mut state = self.state.write().await;
            let model = state
                .models
                .iter_mut()
                .find(|model| model.id == entry_id)
                .ok_or_else(|| "The model is no longer installed.".to_owned())?;
            *model = updated.clone();
        }
        self.flush_state().await?;
        if !apply_restart || !restart_required {
            return Ok(ModelSettingsSaveReport {
                model: updated,
                restart_required,
                restarted: false,
                message: if restart_required {
                    "Settings saved. Apply and restart the model to use load-time changes."
                        .to_owned()
                } else {
                    "Request-time defaults saved and active for the next Lumen Source request."
                        .to_owned()
                },
            });
        }
        if !existing.runtime_capabilities.model_start_stop {
            return Err("This runtime cannot be restarted by Lumen Source.".to_owned());
        }
        let managed_vllm = existing.runtime_id == VLLM_RUNTIME
            && existing.runtime_capabilities.lifecycle
                == Some(crate::runtime_registry::RuntimeLifecycle::Managed);
        let applied_settings = updated
            .model_settings
            .clone()
            .ok_or_else(|| "The model settings were not persisted.".to_owned())?;
        let restart_result = async {
            if managed_vllm {
                self.relaunch_managed_vllm(&existing, &applied_settings)
                    .await?;
                if !existing.running {
                    self.stop(
                        Some(&existing.id),
                        existing.model_id.clone(),
                        existing.target_id.clone(),
                        None,
                    )
                    .await?;
                }
            } else {
                if existing.runtime_id == OLLAMA_RUNTIME {
                    self.apply_ollama_persistent_settings(&existing, &applied_settings)
                        .await?;
                }
                if !existing.running {
                    return Ok::<(), String>(());
                }
                self.stop(
                    Some(&existing.id),
                    existing.model_id.clone(),
                    existing.target_id.clone(),
                    None,
                )
                .await?;
                self.start(
                    Some(&existing.id),
                    existing.model_id.clone(),
                    existing.target_id.clone(),
                    None,
                )
                .await?;
            }
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = restart_result {
            {
                let mut state = self.state.write().await;
                if let Some(model) = state.models.iter_mut().find(|model| model.id == entry_id) {
                    model.model_settings = Some(previous.clone());
                    model.runtime_model_id = existing.runtime_model_id.clone();
                    model.logs.push(format!(
                        "[{}] Settings restart failed; restored the last working configuration.",
                        current_timestamp()
                    ));
                }
            }
            self.flush_state().await?;
            if managed_vllm {
                if self
                    .relaunch_managed_vllm(&existing, &previous)
                    .await
                    .is_ok()
                    && !existing.running
                {
                    let _ = self
                        .stop(
                            Some(&existing.id),
                            existing.model_id.clone(),
                            existing.target_id.clone(),
                            None,
                        )
                        .await;
                }
            } else if existing.running {
                let _ = self
                    .start(
                        Some(&existing.id),
                        existing.model_id.clone(),
                        existing.target_id.clone(),
                        None,
                    )
                    .await;
            }
            return Err(format!(
                "The model could not restart with the new settings. The previous configuration was restored. {error}"
            ));
        }
        self.flush_state().await?;
        let model = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == entry_id)
            .cloned()
            .unwrap_or(updated);
        Ok(ModelSettingsSaveReport {
            model,
            restart_required: false,
            restarted: existing.running,
            message: if existing.running {
                "Settings applied and the model restarted successfully.".to_owned()
            } else {
                "Settings saved. They will apply when the model starts.".to_owned()
            },
        })
    }

    pub async fn managed_vllm_support(&self) -> ManagedVllmSupport {
        managed_vllm::detect_support().await
    }

    async fn managed_vllm_entry(&self, entry_id: Option<&str>) -> Option<PersistedModelEntry> {
        let entry_id = entry_id?;
        self.state
            .read()
            .await
            .models
            .iter()
            .find(|model| {
                model.id == entry_id
                    && model.runtime_id == VLLM_RUNTIME
                    && model.runtime_capabilities.lifecycle
                        == Some(crate::runtime_registry::RuntimeLifecycle::Managed)
            })
            .cloned()
    }

    async fn relaunch_managed_vllm(
        &self,
        entry: &PersistedModelEntry,
        settings: &ModelSettings,
    ) -> Result<(), String> {
        let engine = settings
            .managed_container_engine
            .as_deref()
            .and_then(managed_vllm::parse_engine)
            .ok_or_else(|| "The managed container engine is unavailable.".to_owned())?;
        let port = settings
            .managed_port
            .ok_or_else(|| "The managed vLLM port is unavailable.".to_owned())?;
        let catalog = self.catalog_clone().await?;
        let variant = catalog
            .models
            .iter()
            .flat_map(|model| &model.variants)
            .find(|variant| variant.id == entry.model_id)
            .ok_or_else(|| "The managed vLLM catalog variant is no longer available.".to_owned())?;
        let model_id = variant.hugging_face_model_id.clone().ok_or_else(|| {
            "The catalog variant has no Hugging Face model identifier.".to_owned()
        })?;
        let served_model_name = settings
            .vllm_served_model_name
            .clone()
            .or_else(|| entry.runtime_model_id.clone())
            .ok_or_else(|| "The served vLLM model name is unavailable.".to_owned())?;
        let spec = ManagedVllmSpec {
            entry_id: entry.id.clone(),
            model_id,
            served_model_name,
            port,
            settings: settings.clone(),
            defaults: self.state.read().await.settings.vllm.clone(),
        };
        managed_vllm::validate_spec(&spec)?;
        let token = credential_store::load_runtime_secret_for_account(
            RuntimeSecretKind::HuggingFaceToken,
            "default".to_owned(),
        )
        .await?;
        managed_vllm::launch(engine, &spec, token.as_ref()).await
    }

    async fn apply_ollama_persistent_settings(
        &self,
        entry: &PersistedModelEntry,
        settings: &ModelSettings,
    ) -> Result<(), String> {
        let catalog = self.catalog_clone().await?;
        let (_, variant) = find_variant(&catalog, &entry.model_id)?;
        let runtime_model_id = if settings.ollama_persistent_parameters {
            let derived_name = settings
                .ollama_derived_model_name
                .as_deref()
                .ok_or_else(|| {
                    "A derived model name is required for persistent Ollama parameters.".to_owned()
                })?;
            let runtime = self.runtime_for_target(&entry.target_id).await?;
            runtime
                .ensure_running()
                .await
                .map_err(|error| error.to_string())?;
            runtime
                .create_derived_model(derived_name, &variant.runtime_ref, &chat_options(settings))
                .await
                .map_err(|error| error.to_string())?;
            derived_name.to_owned()
        } else {
            variant.runtime_ref.clone()
        };
        let mut state = self.state.write().await;
        if let Some(model) = state.models.iter_mut().find(|model| model.id == entry.id) {
            model.runtime_model_id = Some(runtime_model_id);
        }
        Ok(())
    }

    pub async fn runtime_migration_options(
        &self,
        entry_id: &str,
    ) -> Result<Vec<RuntimeMigrationOption>, String> {
        let entry = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == entry_id)
            .cloned()
            .ok_or_else(|| "The model is no longer installed.".to_owned())?;
        let target_runtime = if entry.runtime_id == OLLAMA_RUNTIME {
            VLLM_RUNTIME
        } else {
            OLLAMA_RUNTIME
        };
        let catalog = self.catalog_clone().await?;
        let model = catalog.models.iter().find(|model| {
            model
                .variants
                .iter()
                .any(|variant| variant.id == entry.model_id)
        });
        let equivalent = model.and_then(|model| {
            model
                .variants
                .iter()
                .find(|variant| variant.runtime == target_runtime)
        });
        let already_installed = equivalent.is_some_and(|variant| {
            self.state.try_read().is_ok_and(|state| {
                state
                    .models
                    .iter()
                    .any(|model| model.runtime_id == target_runtime && model.model_id == variant.id)
            })
        });
        let managed_support = if target_runtime == VLLM_RUNTIME {
            Some(managed_vllm::detect_support().await)
        } else {
            None
        };
        let requires_hugging_face_token = equivalent.is_some_and(|variant| variant.gated);
        let token_saved = if requires_hugging_face_token {
            credential_store::runtime_secret_is_saved_for_account(
                RuntimeSecretKind::HuggingFaceToken,
                "default".to_owned(),
            )
            .await?
        } else {
            false
        };
        let (available, reason) = match equivalent {
            None => (
                false,
                format!(
                    "The active catalog does not provide an equivalent {target_runtime} variant."
                ),
            ),
            Some(_) if already_installed => (
                false,
                format!("The equivalent {target_runtime} variant is already installed."),
            ),
            Some(_)
                if managed_support
                    .as_ref()
                    .is_some_and(|support| !support.supported) =>
            (
                false,
                managed_support
                    .as_ref()
                    .map(|support| support.message.clone())
                    .unwrap_or_else(|| {
                        "Managed vLLM installation is unavailable on this machine.".to_owned()
                    }),
            ),
            Some(_) => (
                true,
                "An equivalent catalog variant is available. The existing model will remain installed until you remove it explicitly.".to_owned(),
            ),
        };
        Ok(vec![RuntimeMigrationOption {
            runtime_id: target_runtime.to_owned(),
            variant_id: equivalent.map(|variant| variant.id.clone()),
            available,
            reason,
            requires_hugging_face_token,
            token_saved,
        }])
    }

    pub async fn reinstall_with_runtime(
        &self,
        entry_id: &str,
        target_runtime: &str,
    ) -> Result<RuntimeMigrationReport, String> {
        let source = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == entry_id)
            .cloned()
            .ok_or_else(|| "The source model is no longer installed.".to_owned())?;
        if source.runtime_id == target_runtime {
            return Err("Choose a runtime different from the current runtime.".to_owned());
        }
        let catalog = self.catalog_clone().await?;
        let catalog_model = catalog
            .models
            .iter()
            .find(|model| {
                model
                    .variants
                    .iter()
                    .any(|variant| variant.id == source.model_id)
            })
            .ok_or_else(|| {
                "The source model is not tied to the active catalog, so equivalence cannot be verified."
                    .to_owned()
            })?;
        let variant = catalog_model
            .variants
            .iter()
            .find(|variant| variant.runtime == target_runtime)
            .ok_or_else(|| {
                format!(
                    "The active catalog has no equivalent {target_runtime} variant for this model."
                )
            })?
            .clone();
        if self
            .state
            .read()
            .await
            .models
            .iter()
            .any(|model| model.runtime_id == target_runtime && model.model_id == variant.id)
        {
            return Err("The equivalent runtime variant is already installed.".to_owned());
        }
        let replacement = match RuntimeId::parse(target_runtime) {
            Some(RuntimeId::Vllm) => {
                self.install_managed_vllm_variant(catalog_model, &variant, &source)
                    .await?
            }
            Some(RuntimeId::Ollama) => {
                let runtime = self.runtime_for_target("local").await?;
                runtime
                    .ensure_running()
                    .await
                    .map_err(|error| error.to_string())?;
                let reporter = |_: RuntimeProgress| {};
                runtime
                    .pull_model(&variant.runtime_ref, &reporter)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut replacement = PersistedModelEntry {
                    id: Uuid::new_v4().to_string(),
                    name: source.name.clone(),
                    model_id: variant.id.clone(),
                    model_name: catalog_model.display_name.clone(),
                    runtime_id: OLLAMA_RUNTIME.to_owned(),
                    runtime_model_id: Some(variant.runtime_ref.clone()),
                    runtime_capabilities: capabilities_for_catalog_model(
                        &self.runtime_registry,
                        OLLAMA_RUNTIME,
                        catalog_model,
                    ),
                    model_settings: Some(ModelSettings::default()),
                    installation_validation: None,
                    version: catalog
                        .runtimes
                        .iter()
                        .find(|runtime| runtime.id == OLLAMA_RUNTIME)
                        .map(|runtime| runtime.install.version.clone())
                        .unwrap_or_else(|| "unknown".to_owned()),
                    location: "local".to_owned(),
                    target_id: "local".to_owned(),
                    target_name: None,
                    running: false,
                    managed: true,
                    digest: variant.runtime_digest.clone(),
                    size_bytes: Some(variant_size_bytes(&variant)),
                    license_basis: source.license_basis.clone(),
                    license_reference: source.license_reference.clone(),
                    license_acknowledged_at: source.license_acknowledged_at.clone(),
                    license_profile_id: source.license_profile_id.clone(),
                    license_name: source.license_name.clone(),
                    license_url: source.license_url.clone(),
                    license_reviewed_at: source.license_reviewed_at.clone(),
                    license_catalog_version: source.license_catalog_version.clone(),
                    logs: vec![format!(
                        "[{}] Reinstalled with Ollama; the source runtime copy was retained.",
                        current_timestamp()
                    )],
                };
                replacement.running = runtime
                    .status()
                    .await
                    .is_ok_and(|status| matches!(status, CoreRuntimeStatus::Running { models } if models.iter().any(|model| same_ollama_reference(model, &variant.runtime_ref))));
                self.state.write().await.models.push(replacement.clone());
                self.flush_state().await?;
                replacement
            }
            Some(RuntimeId::Dummy) | None => {
                return Err("The requested migration target is not supported.".to_owned())
            }
        };
        Ok(RuntimeMigrationReport {
            replacement,
            source_entry_id: source.id,
            source_can_be_removed: true,
            message: "The replacement was installed and validated. The original copy remains available until you choose to remove it.".to_owned(),
        })
    }

    async fn install_managed_vllm_variant(
        &self,
        catalog_model: &ModelEntry,
        variant: &ModelVariant,
        source: &PersistedModelEntry,
    ) -> Result<PersistedModelEntry, String> {
        let support = managed_vllm::detect_support().await;
        if !support.supported {
            return Err(support.message);
        }
        let engine = support
            .container_engine
            .as_deref()
            .and_then(managed_vllm::parse_engine)
            .ok_or_else(|| "No supported container engine is available.".to_owned())?;
        let hugging_face_model = variant.hugging_face_model_id.clone().ok_or_else(|| {
            "The catalog variant has no Hugging Face model identifier.".to_owned()
        })?;
        let entry_id = Uuid::new_v4().to_string();
        let port = self.reserve_managed_port().await?;
        let defaults = self.state.read().await.settings.vllm.clone();
        let served_name = variant.runtime_ref.clone();
        let inference_task = if catalog_model
            .capabilities
            .iter()
            .any(|capability| capability == "embeddings")
            && !model_supports_chat(catalog_model)
        {
            ModelInferenceTask::Embeddings
        } else {
            ModelInferenceTask::Chat
        };
        let profile = source
            .model_settings
            .as_ref()
            .and_then(|settings| settings.performance_profile)
            .unwrap_or_default();
        let hardware = self.hardware_for_target("local").await?;
        let mut settings = build_performance_profile_report(variant, &hardware, profile).settings;
        settings = ModelSettings {
            performance_profile: Some(profile),
            runtime_management_mode: Some(crate::settings::RuntimeManagementMode::Managed),
            inference_task: Some(inference_task),
            endpoint: Some(format!("http://127.0.0.1:{port}")),
            vllm_model_revision: variant.model_revision.clone(),
            vllm_tokenizer_revision: variant.tokenizer_revision.clone(),
            vllm_served_model_name: Some(served_name.clone()),
            vllm_task: variant.task.clone(),
            vllm_runner: variant.runner.clone(),
            vllm_quantization: variant.quantization.clone(),
            managed_container_engine: Some(engine.as_str().to_owned()),
            managed_port: Some(port),
            ..settings
        };
        let spec = ManagedVllmSpec {
            entry_id: entry_id.clone(),
            model_id: hugging_face_model,
            served_model_name: served_name.clone(),
            port,
            settings: settings.clone(),
            defaults: defaults.clone(),
        };
        managed_vllm::validate_spec(&spec)?;
        let token = credential_store::load_runtime_secret_for_account(
            RuntimeSecretKind::HuggingFaceToken,
            "default".to_owned(),
        )
        .await?;
        if variant.gated && token.is_none() {
            self.managed_ports.lock().await.remove(&port);
            return Err(
                "This gated Hugging Face model requires a token. Enter one in the model migration section and try again."
                    .to_owned(),
            );
        }
        if let Err(error) = managed_vllm::launch(engine, &spec, token.as_ref()).await {
            self.managed_ports.lock().await.remove(&port);
            return Err(error);
        }
        let validation = async {
            let runtime = VllmRuntime::new(
                &format!("http://127.0.0.1:{port}"),
                true,
                std::time::Duration::from_secs(5),
                std::time::Duration::from_secs(settings.request_timeout_seconds.into()),
            )
            .map_err(|error| error.to_string())?;
            let models = runtime.models(None).await.map_err(|error| error.to_string())?;
            if !models.iter().any(|model| model == &served_name) {
                return Err("vLLM did not report the configured served-model identity.".to_owned());
            }
            let mut checks = vec![
                validation_check("runtime", "pass", "The managed vLLM API is healthy."),
                validation_check(
                    "identity",
                    "pass",
                    "vLLM reported the configured served-model identity.",
                ),
            ];
            if inference_task == ModelInferenceTask::Embeddings {
                let embedding = runtime
                    .embeddings(&served_name, "LumenSource validation", None)
                    .await
                    .map_err(|error| error.to_string())?;
                if embedding.is_empty() {
                    return Err("vLLM returned an empty embedding vector.".to_owned());
                }
                checks.push(validation_check(
                    "inference",
                    "pass",
                    &format!(
                        "vLLM returned a non-empty {}-dimension embedding.",
                        embedding.len()
                    ),
                ));
            } else {
                let response_bytes = StdMutex::new(0_usize);
                let reporter = |progress| {
                    if let ChatProgress::Content(content) = progress {
                        if let Ok(mut total) = response_bytes.lock() {
                            *total = total.saturating_add(content.trim().len());
                        }
                    }
                };
                let cancellation = CancellationToken::new();
                let mut options = chat_options(&settings);
                options.temperature = Some(0.0);
                options.max_output_tokens = Some(8);
                runtime
                    .chat_with_options(
                        &served_name,
                        &[ChatMessage {
                            role: "user".to_owned(),
                            content: "Reply with OK.".to_owned(),
                        }],
                        None,
                        &options,
                        &reporter,
                        &cancellation,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                if response_bytes
                    .lock()
                    .map(|total| *total)
                    .unwrap_or_default()
                    == 0
                {
                    return Err("vLLM returned an empty validation response.".to_owned());
                }
                checks.push(validation_check(
                    "inference",
                    "pass",
                    "vLLM returned a non-empty deterministic validation response.",
                ));
            }
            checks.push(validation_check(
                "configuration",
                "warning",
                "The vLLM API does not expose its effective context allocation; the pinned launch arguments were accepted because the server became ready.",
            ));
            checks.push(validation_check(
                "accelerator",
                "warning",
                "The managed container was launched with NVIDIA GPU access; vLLM does not expose per-model allocation through this API.",
            ));
            Ok::<_, String>(InstallationValidationReport {
                passed: true,
                capability: format!("{inference_task:?}").to_ascii_lowercase(),
                runtime_id: VLLM_RUNTIME.to_owned(),
                runtime_model_id: served_name.clone(),
                message: "The managed vLLM model produced a valid response and is ready."
                    .to_owned(),
                accelerator: Some("gpu".to_owned()),
                hardware_summary: Some(hardware_summary(&hardware)),
                effective_context_length: settings.context_length,
                validated_at: chrono::Utc::now().to_rfc3339(),
                running: true,
                settings: settings.clone(),
                checks,
            })
        }
        .await;
        let validation = match validation {
            Ok(validation) => validation,
            Err(error) => {
                let _ = managed_vllm::remove_container(engine, &spec.container_name()).await;
                self.managed_ports.lock().await.remove(&port);
                return Err(format!(
                    "The managed vLLM server started, but inference validation failed: {error}"
                ));
            }
        };
        settings.managed_container_name = Some(spec.container_name());
        let mut validation = validation;
        validation.settings = settings.clone();
        let entry = PersistedModelEntry {
            id: entry_id,
            name: source.name.clone(),
            model_id: variant.id.clone(),
            model_name: catalog_model.display_name.clone(),
            runtime_id: VLLM_RUNTIME.to_owned(),
            runtime_model_id: Some(served_name),
            runtime_capabilities: managed_vllm_capabilities(&self.runtime_registry, inference_task),
            model_settings: Some(settings),
            installation_validation: Some(validation),
            version: defaults.pinned_runtime_version,
            location: "local".to_owned(),
            target_id: "local".to_owned(),
            target_name: None,
            running: true,
            managed: true,
            digest: variant.runtime_digest.clone(),
            size_bytes: Some(variant_size_bytes(variant)),
            license_basis: source.license_basis.clone(),
            license_reference: source.license_reference.clone(),
            license_acknowledged_at: source.license_acknowledged_at.clone(),
            license_profile_id: source.license_profile_id.clone(),
            license_name: source.license_name.clone(),
            license_url: source.license_url.clone(),
            license_reviewed_at: source.license_reviewed_at.clone(),
            license_catalog_version: source.license_catalog_version.clone(),
            logs: vec![format!(
                "[{}] Managed vLLM container installed and validated.",
                current_timestamp()
            )],
        };
        self.state.write().await.models.push(entry.clone());
        self.flush_state().await?;
        Ok(entry)
    }

    async fn reserve_managed_port(&self) -> Result<u16, String> {
        let settings = self.state.read().await.settings.vllm.clone();
        let mut reserved = self.managed_ports.lock().await;
        for port in settings.managed_port_start..=settings.managed_port_end {
            if reserved.contains(&port) {
                continue;
            }
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                reserved.insert(port);
                return Ok(port);
            }
        }
        Err("No free port is available in the managed vLLM port range.".to_owned())
    }

    pub async fn runtime_diagnostics(&self, entry_id: &str) -> Result<RuntimeDiagnostics, String> {
        let entry = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == entry_id)
            .cloned()
            .ok_or_else(|| "The model is no longer installed.".to_owned())?;
        let settings = entry.model_settings.clone().unwrap_or_default();
        let lifecycle = entry
            .runtime_capabilities
            .lifecycle
            .map(|lifecycle| format!("{lifecycle:?}").to_ascii_lowercase())
            .unwrap_or_else(|| "unknown".to_owned());
        let managed_container = if entry.runtime_id == VLLM_RUNTIME && lifecycle == "managed" {
            settings
                .managed_container_engine
                .as_deref()
                .and_then(managed_vllm::parse_engine)
                .zip(settings.managed_container_name.as_deref())
        } else {
            None
        };
        let recent_logs = if let Some((engine, name)) = managed_container {
            managed_vllm::logs(engine, name, 50)
                .await
                .unwrap_or_default()
        } else {
            entry.logs.iter().rev().take(50).cloned().collect()
        };
        Ok(RuntimeDiagnostics {
            runtime_id: entry.runtime_id,
            version: entry.version,
            health: if entry.running {
                "healthy".to_owned()
            } else {
                "stopped or unavailable".to_owned()
            },
            lifecycle,
            endpoint: settings.endpoint,
            effective_context_length: settings.context_length,
            effective_keep_alive: settings.keep_alive,
            managed_container_engine: settings.managed_container_engine,
            managed_container_name: settings.managed_container_name,
            managed_port: settings.managed_port,
            recent_logs,
        })
    }

    pub async fn delete_managed_vllm_caches(&self, confirmed: bool) -> Result<(), String> {
        let support = managed_vllm::detect_support().await;
        let engine = support
            .container_engine
            .as_deref()
            .and_then(managed_vllm::parse_engine)
            .ok_or_else(|| "No supported container engine is available.".to_owned())?;
        managed_vllm::delete_caches(engine, confirmed).await
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
        let supports_runtime =
            |runtime: &str| supports_catalog_runtime(&self.runtime_registry, runtime, target_id);
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
                labels: recommendation_labels(model, variant, true),
                estimated_loaded_memory_min_bytes: estimated_loaded_memory(variant).0,
                estimated_loaded_memory_max_bytes: estimated_loaded_memory(variant).1,
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
                labels: recommendation_labels(model, variant, false),
                estimated_loaded_memory_min_bytes: estimated_loaded_memory(variant).0,
                estimated_loaded_memory_max_bytes: estimated_loaded_memory(variant).1,
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
            if !self
                .runtime_registry
                .resolve_name(&variant.runtime)
                .is_some_and(|runtime| runtime.capabilities.remote_connection)
            {
                return Err(format!(
                    "{} catalog models cannot be installed on remote targets",
                    variant.runtime
                ));
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
        let profile = self
            .performance_profile(&variant_id, &target_id, options.performance_profile)
            .await?;
        if !profile.fits_detected_memory {
            return Err(profile.warnings.first().cloned().unwrap_or_else(|| {
                "The selected performance profile exceeds the detected memory budget.".to_owned()
            }));
        }
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

    pub async fn performance_profile(
        &self,
        variant_id: &str,
        target_id: &str,
        profile: PerformanceProfile,
    ) -> Result<PerformanceProfileReport, String> {
        let catalog = self.catalog_clone().await?;
        let (_, variant) = find_variant(&catalog, variant_id)?;
        ensure_supported_runtime(variant)?;
        let hardware = self.hardware_for_target(target_id).await?;
        Ok(build_performance_profile_report(
            variant, &hardware, profile,
        ))
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
        let was_already_installed;
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

        if target_id != "local"
            && !self
                .runtime_registry
                .resolve_name(&variant.runtime)
                .is_some_and(|runtime| runtime.capabilities.remote_connection)
        {
            return Err(format!(
                "{} catalog models cannot be installed on remote targets",
                variant.runtime
            ));
        }
        if variant.runtime == DUMMY_RUNTIME {
            was_already_installed = self
                .dummy_runtime
                .installed_models()
                .await
                .is_ok_and(|models| models.iter().any(|model| model.name == runtime_ref));
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
            was_already_installed = self.runtime.installed_models().await.is_ok_and(|models| {
                models
                    .iter()
                    .any(|model| same_ollama_reference(&model.name, &runtime_ref))
            });
            self.runtime
                .pull_model_cancellable(&runtime_ref, &reporter, cancellation)
                .await
                .map_err(|error| error.to_string())?;
        } else {
            let runtime = self.runtime_for_target(target_id).await?;
            was_already_installed = runtime.installed_models().await.is_ok_and(|models| {
                models
                    .iter()
                    .any(|model| same_ollama_reference(&model.name, &runtime_ref))
            });
            runtime
                .pull_model_cancellable(&runtime_ref, &reporter, cancellation)
                .await
                .map_err(|error| error.to_string())?;
        }
        cancellation.check().map_err(|error| error.to_string())?;
        *self.installed_model.write().await = Some(runtime_ref);
        *self.selected_runtime.write().await = Some(variant.runtime.clone());
        *self.recent_install.write().await = Some(RecentInstall {
            variant_id: variant_id.clone(),
            target_id: target_id.to_owned(),
            was_already_installed,
        });
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
        entry_id: Option<&str>,
        variant_id: String,
        target_id: String,
        password: Option<Zeroizing<String>>,
    ) -> Result<RuntimeStatus, String> {
        if let Some(entry) = self.managed_vllm_entry(entry_id).await {
            let settings = entry.model_settings.as_ref().ok_or_else(|| {
                "The managed vLLM model has incomplete deployment settings.".to_owned()
            })?;
            let engine = settings
                .managed_container_engine
                .as_deref()
                .and_then(managed_vllm::parse_engine)
                .ok_or_else(|| "The managed container engine is unavailable.".to_owned())?;
            let name = settings
                .managed_container_name
                .as_deref()
                .ok_or_else(|| "The managed vLLM container identity is unavailable.".to_owned())?;
            managed_vllm::start(engine, name).await?;
            let port = settings
                .managed_port
                .ok_or_else(|| "The managed vLLM port is unavailable.".to_owned())?;
            let served_model = settings
                .vllm_served_model_name
                .as_deref()
                .or(entry.runtime_model_id.as_deref())
                .ok_or_else(|| "The served vLLM model name is unavailable.".to_owned())?;
            if let Err(error) = managed_vllm::wait_until_healthy(port, served_model, 180).await {
                let logs = managed_vllm::logs(engine, name, 30)
                    .await
                    .unwrap_or_default();
                return Err(format!(
                    "{error} {}",
                    logs.last().cloned().unwrap_or_else(|| {
                        "Inspect the managed-container logs for the model-loading error.".to_owned()
                    })
                ));
            }
            let mut state = self.state.write().await;
            if let Some(model) = state.models.iter_mut().find(|model| model.id == entry.id) {
                model.running = true;
                model.logs.push(format!(
                    "[{}] Managed vLLM container started.",
                    current_timestamp()
                ));
            }
            drop(state);
            self.flush_state().await?;
            return Ok(RuntimeStatus {
                state: "running".to_owned(),
                model_id: entry.runtime_model_id,
                message: Some("Managed vLLM container is running on loopback.".to_owned()),
            });
        }
        let catalog = self.catalog_clone().await?;
        let (model, variant) = find_variant(&catalog, &variant_id)?;
        let runtime_ref = if variant.runtime == OLLAMA_RUNTIME {
            match entry_id {
                Some(entry_id) => self
                    .state
                    .read()
                    .await
                    .models
                    .iter()
                    .find(|entry| entry.id == entry_id)
                    .and_then(|entry| entry.runtime_model_id.clone())
                    .unwrap_or_else(|| variant.runtime_ref.clone()),
                None => variant.runtime_ref.clone(),
            }
        } else {
            variant.runtime_ref.clone()
        };
        let telemetry_model_id = model.id.clone();
        let telemetry_variant_id = variant.id.clone();
        ensure_supported_runtime(variant)?;
        let result: Result<RuntimeStatus, String> = async {
            if target_id != "local"
                && !self
                    .runtime_registry
                    .resolve_name(&variant.runtime)
                    .is_some_and(|runtime| runtime.capabilities.remote_connection)
            {
                return Err(format!(
                    "{} catalog models cannot be started on remote targets",
                    variant.runtime
                ));
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
                        .start_embedding(&runtime_ref)
                        .await
                        .map_err(|error| error.to_string())?;
                } else {
                    runtime
                        .start(&runtime_ref)
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
            *self.installed_model.write().await = Some(runtime_ref.clone());
            *self.selected_runtime.write().await = Some(variant.runtime.clone());
            self.persist_state().await?;
            Ok(RuntimeStatus {
                state: "running".to_owned(),
                model_id: Some(runtime_ref.clone()),
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
        entry_id: Option<&str>,
        variant_id: String,
        target_id: String,
        password: Option<Zeroizing<String>>,
    ) -> Result<RuntimeStatus, String> {
        if let Some(entry) = self.managed_vllm_entry(entry_id).await {
            let settings = entry.model_settings.as_ref().ok_or_else(|| {
                "The managed vLLM model has incomplete deployment settings.".to_owned()
            })?;
            let engine = settings
                .managed_container_engine
                .as_deref()
                .and_then(managed_vllm::parse_engine)
                .ok_or_else(|| "The managed container engine is unavailable.".to_owned())?;
            let name = settings
                .managed_container_name
                .as_deref()
                .ok_or_else(|| "The managed vLLM container identity is unavailable.".to_owned())?;
            managed_vllm::stop(engine, name).await?;
            let mut state = self.state.write().await;
            if let Some(model) = state.models.iter_mut().find(|model| model.id == entry.id) {
                model.running = false;
                model.logs.push(format!(
                    "[{}] Managed vLLM container stopped.",
                    current_timestamp()
                ));
            }
            drop(state);
            self.flush_state().await?;
            return Ok(RuntimeStatus::stopped());
        }
        let catalog = self.catalog_clone().await?;
        let (model, variant) = find_variant(&catalog, &variant_id)?;
        let runtime_ref = if variant.runtime == OLLAMA_RUNTIME {
            match entry_id {
                Some(entry_id) => self
                    .state
                    .read()
                    .await
                    .models
                    .iter()
                    .find(|entry| entry.id == entry_id)
                    .and_then(|entry| entry.runtime_model_id.clone())
                    .unwrap_or_else(|| variant.runtime_ref.clone()),
                None => variant.runtime_ref.clone(),
            }
        } else {
            variant.runtime_ref.clone()
        };
        ensure_supported_runtime(variant)?;
        {
            let active = self.active_chat.lock().await;
            if active
                .as_ref()
                .is_some_and(|chat| same_ollama_reference(&chat.runtime_model_id, &runtime_ref))
            {
                if let Some(chat) = active.as_ref() {
                    chat.cancellation.cancel();
                }
            }
        }
        if target_id != "local"
            && !self
                .runtime_registry
                .resolve_name(&variant.runtime)
                .is_some_and(|runtime| runtime.capabilities.remote_connection)
        {
            return Err(format!(
                "{} catalog models cannot be stopped on remote targets",
                variant.runtime
            ));
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
                    .stop_embedding(&runtime_ref)
                    .await
                    .map_err(|error| error.to_string())?;
            } else {
                runtime
                    .stop(&runtime_ref)
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

    pub async fn validate_installation(
        &self,
        variant_id: &str,
        target_id: &str,
        profile: PerformanceProfile,
        leave_running: bool,
    ) -> Result<InstallationValidationReport, String> {
        let catalog = self.catalog_clone().await?;
        let (model, variant) = find_variant(&catalog, variant_id)?;
        ensure_supported_runtime(variant)?;
        let profile_report = self
            .performance_profile(variant_id, target_id, profile)
            .await?;
        let validation_hardware = profile_report.hardware_summary.clone();
        let settings = profile_report.settings;
        let capability = if model_is_embedding_only(model) {
            "embeddings"
        } else {
            "chat"
        }
        .to_owned();
        let mut checks = Vec::new();

        if variant.runtime == DUMMY_RUNTIME {
            self.dummy_runtime
                .start(&variant.runtime_ref)
                .await
                .map_err(|error| error.to_string())?;
            checks.push(validation_check(
                "runtime",
                "pass",
                "The simulated test runtime is healthy.",
            ));
            checks.push(validation_check(
                "identity",
                "pass",
                "The simulated runtime contains the selected test model.",
            ));
            checks.push(validation_check(
                "inference",
                "pass",
                "The simulated lifecycle validation completed.",
            ));
            if !leave_running {
                self.dummy_runtime
                    .stop(&variant.runtime_ref)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            return Ok(InstallationValidationReport {
                passed: true,
                capability: "simulation".to_owned(),
                runtime_id: DUMMY_RUNTIME.to_owned(),
                runtime_model_id: variant.runtime_ref.clone(),
                message: "The test-runtime lifecycle was validated.".to_owned(),
                accelerator: None,
                hardware_summary: Some(validation_hardware),
                effective_context_length: settings.context_length,
                validated_at: chrono::Utc::now().to_rfc3339(),
                running: leave_running,
                settings,
                checks,
            });
        }

        if variant.runtime != OLLAMA_RUNTIME {
            return Err(format!(
                "Post-install validation is not implemented for the {} runtime.",
                variant.runtime
            ));
        }
        let runtime = self.runtime_for_target(target_id).await?;
        if let Err(error) = runtime.health().await {
            checks.push(validation_check(
                "runtime",
                "fail",
                &format!("Ollama did not pass its health check: {error}"),
            ));
            return Ok(failed_validation_report(
                variant,
                capability,
                settings,
                checks,
                "Ollama is unavailable, so the model could not be validated.",
            ));
        }
        checks.push(validation_check(
            "runtime",
            "pass",
            "Ollama responded to its health check.",
        ));

        let installed = match runtime.installed_models().await {
            Ok(installed) => installed,
            Err(error) => {
                checks.push(validation_check(
                    "identity",
                    "fail",
                    &format!("The installed-model list could not be read: {error}"),
                ));
                return Ok(failed_validation_report(
                    variant,
                    capability,
                    settings,
                    checks,
                    "The runtime model identity could not be verified.",
                ));
            }
        };
        if !installed
            .iter()
            .any(|installed| same_ollama_reference(&installed.name, &variant.runtime_ref))
        {
            checks.push(validation_check(
                "identity",
                "fail",
                "Ollama did not report the catalog-pinned model reference after installation.",
            ));
            return Ok(failed_validation_report(
                variant,
                capability,
                settings,
                checks,
                "The installed model identity did not match the selected catalog model.",
            ));
        }
        checks.push(validation_check(
            "identity",
            "pass",
            "Ollama reported the expected model identity.",
        ));

        let inference_result = if model_is_embedding_only(model) {
            runtime
                .embedding_dimensions(&variant.runtime_ref, -1)
                .await
                .map(|dimensions| {
                    checks.push(validation_check(
                        "inference",
                        "pass",
                        &format!("Ollama returned a non-empty {dimensions}-dimension embedding."),
                    ));
                })
        } else {
            let response_bytes = StdMutex::new(0_usize);
            let reporter = |progress| {
                if let ChatProgress::Content(content) = progress {
                    if let Ok(mut total) = response_bytes.lock() {
                        *total = total.saturating_add(content.trim().len());
                    }
                }
            };
            let mut options = chat_options(&settings);
            options.temperature = Some(0.0);
            options.max_output_tokens = Some(8);
            options.keep_alive = Some("-1".to_owned());
            let cancellation = CancellationToken::new();
            runtime
                .chat_with_options_cancellable(
                    &variant.runtime_ref,
                    &[ChatMessage {
                        role: "user".to_owned(),
                        content: "Reply with OK.".to_owned(),
                    }],
                    &options,
                    &reporter,
                    &cancellation,
                )
                .await
                .and_then(|()| {
                    let response_bytes = response_bytes
                        .lock()
                        .map(|total| *total)
                        .unwrap_or_default();
                    if response_bytes == 0 {
                        Err(RuntimeError::Remote(
                            "Ollama returned an empty validation response".to_owned(),
                        ))
                    } else {
                        checks.push(validation_check(
                            "inference",
                            "pass",
                            "Ollama returned a non-empty deterministic validation response.",
                        ));
                        Ok(())
                    }
                })
        };

        if let Err(error) = inference_result {
            checks.push(validation_check(
                "inference",
                "fail",
                &format!("The validation request failed: {error}"),
            ));
            let _ = if model_is_embedding_only(model) {
                runtime.stop_embedding(&variant.runtime_ref).await
            } else {
                runtime.stop(&variant.runtime_ref).await
            };
            return Ok(failed_validation_report(
                variant,
                capability,
                settings,
                checks,
                "The model was downloaded, but it did not produce a valid response.",
            ));
        }

        let allocation = runtime
            .model_allocation(&variant.runtime_ref)
            .await
            .unwrap_or(None);
        let accelerator = allocation.as_ref().map(|allocation| {
            if allocation.vram_memory_bytes > 0
                && allocation.total_memory_bytes > allocation.vram_memory_bytes
            {
                "mixed"
            } else if allocation.vram_memory_bytes > 0 {
                "gpu"
            } else {
                "cpu"
            }
            .to_owned()
        });
        let effective_context_length = allocation
            .as_ref()
            .and_then(|allocation| allocation.context_length)
            .and_then(|context| u32::try_from(context).ok());
        if let Some(context) = effective_context_length {
            let status = if settings.context_length == Some(context) {
                "pass"
            } else {
                "warning"
            };
            checks.push(validation_check(
                "configuration",
                status,
                &format!(
                    "Ollama reports a {context}-token context allocation; Lumen Source requested {}.",
                    settings
                        .context_length
                        .map_or_else(|| "the runtime default".to_owned(), |value| value.to_string())
                ),
            ));
        } else {
            checks.push(validation_check(
                "configuration",
                "warning",
                "Ollama did not expose the effective context allocation. Inference still passed.",
            ));
        }
        checks.push(validation_check(
            "accelerator",
            if accelerator.is_some() {
                "pass"
            } else {
                "warning"
            },
            accelerator.as_deref().map_or(
                "Ollama did not expose CPU/GPU allocation metrics; this does not affect readiness.",
                |value| match value {
                    "gpu" => "Ollama reports that the model is allocated on the GPU.",
                    "mixed" => "Ollama reports a mixed GPU and system-memory allocation.",
                    _ => "Ollama reports a system-memory/CPU allocation.",
                },
            ),
        ));

        if !leave_running {
            let stop_result = if model_is_embedding_only(model) {
                runtime.stop_embedding(&variant.runtime_ref).await
            } else {
                runtime.stop(&variant.runtime_ref).await
            };
            if let Err(error) = stop_result {
                checks.push(validation_check(
                    "unload",
                    "warning",
                    &format!("Validation passed, but the temporary model load could not be released: {error}"),
                ));
            }
        }

        Ok(InstallationValidationReport {
            passed: true,
            capability,
            runtime_id: OLLAMA_RUNTIME.to_owned(),
            runtime_model_id: variant.runtime_ref.clone(),
            message: "The installed model produced a valid response and is ready.".to_owned(),
            accelerator,
            hardware_summary: Some(validation_hardware),
            effective_context_length,
            validated_at: chrono::Utc::now().to_rfc3339(),
            running: leave_running,
            settings,
            checks,
        })
    }

    pub async fn remove_incomplete_install(
        &self,
        variant_id: &str,
        target_id: &str,
        confirmed: bool,
    ) -> Result<(), String> {
        if !confirmed {
            return Err("Confirm removal of the incomplete installation.".to_owned());
        }
        let recent = self
            .recent_install
            .read()
            .await
            .clone()
            .filter(|install| {
                install.variant_id == variant_id && install.target_id == target_id
            })
            .ok_or_else(|| {
                "Lumen Source can only remove the model downloaded by the current installation attempt."
                    .to_owned()
            })?;
        if recent.was_already_installed {
            return Err(
                "This model existed before the current attempt, so Lumen Source will not delete it."
                    .to_owned(),
            );
        }
        if self
            .state
            .read()
            .await
            .models
            .iter()
            .any(|entry| entry.model_id == variant_id && entry.target_id == target_id)
        {
            return Err(
                "The model is already part of the library and cannot be removed as incomplete."
                    .to_owned(),
            );
        }
        let catalog = self.catalog_clone().await?;
        let (_, variant) = find_variant(&catalog, variant_id)?;
        match variant.runtime.as_str() {
            DUMMY_RUNTIME => self
                .dummy_runtime
                .delete_model(&variant.runtime_ref)
                .await
                .map_err(|error| error.to_string())?,
            OLLAMA_RUNTIME => self
                .runtime_for_target(target_id)
                .await?
                .delete_model(&variant.runtime_ref)
                .await
                .map_err(|error| error.to_string())?,
            runtime => {
                return Err(format!(
                    "Incomplete-install cleanup is not supported for {runtime}."
                ))
            }
        }
        if self
            .installed_model
            .read()
            .await
            .as_deref()
            .is_some_and(|installed| same_ollama_reference(installed, &variant.runtime_ref))
        {
            *self.installed_model.write().await = None;
            *self.selected_runtime.write().await = None;
        }
        *self.recent_install.write().await = None;
        self.persist_state().await
    }

    pub async fn performance(
        &self,
        entry_id: &str,
        model_id: &str,
        runtime_model_id: &str,
        target_id: &str,
    ) -> Result<PerformanceSnapshot, String> {
        if let Some(entry) = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == entry_id && model.runtime_id == VLLM_RUNTIME)
            .cloned()
        {
            let config = vllm_config_for_entry(&entry)
                .ok_or_else(|| "This vLLM model has incomplete connection settings.".to_owned())?;
            let key = credential_store::load_runtime_secret_for_account(
                RuntimeSecretKind::VllmApiKey,
                entry.id,
            )
            .await?;
            let state = match vllm_runtime(&config)
                .map_err(|error| error.to_string())?
                .models(key.as_deref().map(String::as_str))
                .await
            {
                Ok(models) if models.iter().any(|model| model == runtime_model_id) => "running",
                Ok(_) => "stopped",
                Err(_) => "unavailable",
            };
            return Ok(PerformanceSnapshot {
                model_id: runtime_model_id.to_owned(),
                state: state.to_owned(),
                sampled_at_unix_ms: current_unix_time_ms(),
                allocated_memory_bytes: 0,
                allocated_vram_bytes: 0,
                allocated_system_memory_bytes: 0,
                context_length: None,
            });
        }
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
        entry_id: &str,
        model_id: &str,
        runtime_model_id: &str,
        target_id: &str,
        messages: Vec<ChatMessage>,
        reporter: &(dyn Fn(ChatEvent) + Send + Sync),
    ) -> Result<(), String> {
        validate_chat_messages(&messages)?;
        if let Some(entry) = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == entry_id && model.runtime_id == VLLM_RUNTIME)
            .cloned()
        {
            return self
                .chat_with_vllm(entry, runtime_model_id, messages, reporter)
                .await;
        }
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
        let persisted_runtime_matches = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|entry| entry.id == entry_id)
            .and_then(|entry| entry.runtime_model_id.as_deref())
            .is_some_and(|persisted| same_ollama_reference(persisted, runtime_model_id));
        if !same_ollama_reference(&variant.runtime_ref, runtime_model_id)
            && !persisted_runtime_matches
        {
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
        let options = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|entry| entry.id == entry_id)
            .and_then(|entry| entry.model_settings.as_ref())
            .map(chat_options)
            .unwrap_or_default();
        let result = runtime
            .chat_with_options_cancellable(
                runtime_model_id,
                &messages,
                &options,
                &chat_reporter,
                &cancellation,
            )
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

    async fn chat_with_vllm(
        &self,
        entry: PersistedModelEntry,
        runtime_model_id: &str,
        messages: Vec<ChatMessage>,
        reporter: &(dyn Fn(ChatEvent) + Send + Sync),
    ) -> Result<(), String> {
        if !entry.runtime_capabilities.chat {
            return Err("This vLLM endpoint does not expose chat completions.".to_owned());
        }
        if entry.runtime_model_id.as_deref() != Some(runtime_model_id) {
            return Err("The served vLLM model identifier has changed.".to_owned());
        }
        let config = vllm_config_for_entry(&entry)
            .ok_or_else(|| "This vLLM model has incomplete connection settings.".to_owned())?;
        let key = credential_store::load_runtime_secret_for_account(
            RuntimeSecretKind::VllmApiKey,
            entry.id.clone(),
        )
        .await?;
        let runtime = vllm_runtime(&config).map_err(|error| error.to_string())?;
        let options = entry
            .model_settings
            .as_ref()
            .map(chat_options)
            .unwrap_or_default();
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
            .chat_with_options(
                runtime_model_id,
                &messages,
                key.as_deref().map(String::as_str),
                &options,
                &chat_reporter,
                &cancellation,
            )
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
            model_id: "external-vllm".to_owned(),
            variant_id: "external".to_owned(),
            deployment: "local".to_owned(),
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
        entry_id: &str,
        model_id: &str,
        runtime_model_id: &str,
        target_id: &str,
    ) -> Result<EndpointDetails, String> {
        if runtime_model_id.trim().is_empty() {
            return Err("The model does not expose a runtime model identifier".to_owned());
        }
        if let Some(entry) = self
            .state
            .read()
            .await
            .models
            .iter()
            .find(|model| model.id == entry_id && model.runtime_id == VLLM_RUNTIME)
            .cloned()
        {
            let config = vllm_config_for_entry(&entry)
                .ok_or_else(|| "This vLLM model has incomplete connection settings.".to_owned())?;
            let runtime = vllm_runtime(&config).map_err(|error| error.to_string())?;
            let mut details = endpoint_details(
                runtime.endpoint(),
                runtime_model_id.to_owned(),
                true,
                entry.runtime_capabilities.chat,
                entry.runtime_capabilities.embeddings,
            )?;
            details.api_key_required = self.vllm_credential_status(&entry.id).await?;
            return Ok(details);
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
        let (mut vllm_persisted, local_persisted): (Vec<_>, Vec<_>) = local_persisted
            .into_iter()
            .partition(|model| model.runtime_id == VLLM_RUNTIME);
        self.refresh_vllm_models(&mut vllm_persisted).await;
        for model in &mut remote_persisted {
            model.runtime_id = OLLAMA_RUNTIME.to_owned();
            model.runtime_capabilities = self
                .runtime_registry
                .resolve(RuntimeId::Ollama)
                .capabilities
                .clone();
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
                let mut models = with_remote_models(models, remote_persisted);
                models.extend(vllm_persisted);
                models.sort_by_key(|model| model.name.to_ascii_lowercase());
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
        let mut models = with_remote_models(models, remote_persisted);
        models.extend(vllm_persisted);
        models.sort_by_key(|model| model.name.to_ascii_lowercase());
        self.replace_models(models.clone()).await?;
        Ok(models)
    }

    async fn refresh_vllm_models(&self, models: &mut [PersistedModelEntry]) {
        for model in models {
            let settings = model.model_settings.clone().unwrap_or_default();
            if settings.runtime_management_mode
                == Some(crate::settings::RuntimeManagementMode::Managed)
            {
                let task = settings.inference_task.unwrap_or_default();
                model.runtime_capabilities =
                    managed_vllm_capabilities(&self.runtime_registry, task);
                model.running = match (
                    settings
                        .managed_container_engine
                        .as_deref()
                        .and_then(managed_vllm::parse_engine),
                    settings.managed_container_name.as_deref(),
                ) {
                    (Some(engine), Some(name)) => managed_vllm::is_running(engine, name)
                        .await
                        .unwrap_or(false),
                    _ => false,
                };
                continue;
            }
            let Some(config) = vllm_config_for_entry(model) else {
                model.running = false;
                continue;
            };
            model.runtime_capabilities =
                external_vllm_capabilities(&self.runtime_registry, config.inference_task);
            let key = credential_store::load_runtime_secret_for_account(
                RuntimeSecretKind::VllmApiKey,
                model.id.clone(),
            )
            .await
            .ok()
            .flatten();
            model.running = match vllm_runtime(&config) {
                Ok(runtime) => runtime
                    .models(key.as_deref().map(String::as_str))
                    .await
                    .is_ok_and(|served| {
                        model
                            .runtime_model_id
                            .as_ref()
                            .is_some_and(|expected| served.contains(expected))
                    }),
                Err(_) => false,
            };
        }
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
        if entry.runtime_id == VLLM_RUNTIME {
            if entry.runtime_capabilities.lifecycle
                == Some(crate::runtime_registry::RuntimeLifecycle::Managed)
            {
                let settings = entry.model_settings.as_ref().ok_or_else(|| {
                    "The managed vLLM model has incomplete deployment settings.".to_owned()
                })?;
                let engine = settings
                    .managed_container_engine
                    .as_deref()
                    .and_then(managed_vllm::parse_engine)
                    .ok_or_else(|| "The managed container engine is unavailable.".to_owned())?;
                let name = settings.managed_container_name.as_deref().ok_or_else(|| {
                    "The managed vLLM container identity is unavailable.".to_owned()
                })?;
                managed_vllm::remove_container(engine, name).await?;
                if let Some(port) = settings.managed_port {
                    self.managed_ports.lock().await.remove(&port);
                }
            }
            credential_store::delete_runtime_secret_for_account(
                RuntimeSecretKind::VllmApiKey,
                entry.id.clone(),
            )
            .await?;
            let models = {
                let mut state = self.state.write().await;
                state.models.retain(|model| model.id != entry.id);
                state.models.clone()
            };
            self.flush_state().await?;
            return Ok(models);
        }
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
                    model.runtime_id != entry.runtime_id
                        || model.target_id != target_id
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
                if model.managed && model.model_settings.is_none() {
                    model.model_settings = Some(ModelSettings::default());
                }
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

fn vllm_runtime(config: &ExternalVllmConfig) -> Result<VllmRuntime, RuntimeError> {
    VllmRuntime::new(
        &config.endpoint,
        config.verify_tls,
        std::time::Duration::from_secs(config.connection_timeout_seconds.into()),
        std::time::Duration::from_secs(config.request_timeout_seconds.into()),
    )
}

fn vllm_config_for_entry(entry: &PersistedModelEntry) -> Option<ExternalVllmConfig> {
    let settings = entry.model_settings.as_ref()?;
    Some(ExternalVllmConfig {
        endpoint: settings.endpoint.clone()?,
        served_model: entry.runtime_model_id.clone().unwrap_or_default(),
        inference_task: settings.inference_task.unwrap_or_default(),
        verify_tls: settings.verify_tls,
        connection_timeout_seconds: settings.connection_timeout_seconds,
        request_timeout_seconds: settings.request_timeout_seconds,
    })
}

fn chat_options(settings: &crate::settings::ModelSettings) -> ChatOptions {
    ChatOptions {
        system_prompt: settings.system_prompt.clone(),
        context_length: settings.context_length,
        temperature: settings.temperature,
        max_output_tokens: settings.max_output_tokens,
        top_p: settings.top_p,
        top_k: settings.top_k,
        min_p: settings.min_p,
        repetition_penalty: settings.repetition_penalty,
        seed: settings.seed,
        stop_sequences: settings.stop_sequences.clone(),
        structured_output: settings.structured_output,
        reasoning_level: settings
            .reasoning_level
            .map(|level| format!("{level:?}").to_ascii_lowercase()),
        keep_alive: settings.keep_alive.clone(),
    }
}

fn load_settings_changed(previous: &ModelSettings, next: &ModelSettings, runtime_id: &str) -> bool {
    if previous.context_length != next.context_length
        || previous.load_on_startup != next.load_on_startup
        || previous.preferred_accelerator != next.preferred_accelerator
    {
        return true;
    }
    match RuntimeId::parse(runtime_id) {
        Some(RuntimeId::Ollama) => {
            previous.ollama_derived_model_name != next.ollama_derived_model_name
                || previous.ollama_persistent_parameters != next.ollama_persistent_parameters
                || ((previous.ollama_persistent_parameters || next.ollama_persistent_parameters)
                    && ollama_persistent_settings_changed(previous, next))
        }
        Some(RuntimeId::Vllm) => external_engine_settings_changed(previous, next),
        Some(RuntimeId::Dummy) | None => false,
    }
}

fn ollama_persistent_settings_changed(previous: &ModelSettings, next: &ModelSettings) -> bool {
    previous.system_prompt != next.system_prompt
        || previous.temperature != next.temperature
        || previous.max_output_tokens != next.max_output_tokens
        || previous.top_p != next.top_p
        || previous.top_k != next.top_k
        || previous.min_p != next.min_p
        || previous.repetition_penalty != next.repetition_penalty
        || previous.seed != next.seed
        || previous.stop_sequences != next.stop_sequences
}

fn external_engine_settings_changed(previous: &ModelSettings, next: &ModelSettings) -> bool {
    previous.runtime_management_mode != next.runtime_management_mode
        || previous.inference_task != next.inference_task
        || previous.endpoint != next.endpoint
        || previous.verify_tls != next.verify_tls
        || previous.connection_timeout_seconds != next.connection_timeout_seconds
        || previous.request_timeout_seconds != next.request_timeout_seconds
        || previous.context_length != next.context_length
        || previous.keep_alive != next.keep_alive
        || previous.load_on_startup != next.load_on_startup
        || previous.preferred_accelerator != next.preferred_accelerator
        || previous.vllm_model_revision != next.vllm_model_revision
        || previous.vllm_tokenizer_revision != next.vllm_tokenizer_revision
        || previous.vllm_served_model_name != next.vllm_served_model_name
        || previous.vllm_task != next.vllm_task
        || previous.vllm_runner != next.vllm_runner
        || previous.vllm_weight_dtype != next.vllm_weight_dtype
        || previous.vllm_quantization != next.vllm_quantization
        || previous.vllm_gpu_memory_utilization != next.vllm_gpu_memory_utilization
        || previous.vllm_max_concurrent_sequences != next.vllm_max_concurrent_sequences
        || previous.vllm_prefix_caching != next.vllm_prefix_caching
        || previous.vllm_kv_cache_dtype != next.vllm_kv_cache_dtype
        || previous.vllm_cpu_offload_gib != next.vllm_cpu_offload_gib
        || previous.vllm_tensor_parallel_size != next.vllm_tensor_parallel_size
        || previous.vllm_pipeline_parallel_size != next.vllm_pipeline_parallel_size
}

fn current_timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn external_vllm_capabilities(
    registry: &RuntimeRegistry,
    task: ModelInferenceTask,
) -> crate::runtime_registry::RuntimeCapabilities {
    let mut capabilities = registry.resolve(RuntimeId::Vllm).capabilities.clone();
    capabilities.chat = task == ModelInferenceTask::Chat;
    capabilities.embeddings = task == ModelInferenceTask::Embeddings;
    capabilities.pooling = false;
    capabilities
}

fn managed_vllm_capabilities(
    registry: &RuntimeRegistry,
    task: ModelInferenceTask,
) -> crate::runtime_registry::RuntimeCapabilities {
    let mut capabilities = external_vllm_capabilities(registry, task);
    capabilities.managed_model_storage = true;
    capabilities.multiple_models = false;
    capabilities.model_start_stop = true;
    capabilities.global_configuration = true;
    capabilities.artifact_acquisition = true;
    capabilities.lifecycle = Some(crate::runtime_registry::RuntimeLifecycle::Managed);
    capabilities
}

fn capabilities_for_catalog_model(
    registry: &RuntimeRegistry,
    runtime_id: &str,
    model: &ModelEntry,
) -> crate::runtime_registry::RuntimeCapabilities {
    let mut capabilities = registry
        .resolve_name(runtime_id)
        .map(|runtime| runtime.capabilities.clone())
        .unwrap_or_default();
    capabilities.chat = model_supports_chat(model);
    capabilities.embeddings = model_is_embedding_only(model);
    capabilities
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

fn build_performance_profile_report(
    variant: &ModelVariant,
    hardware: &HardwareFacts,
    profile: PerformanceProfile,
) -> PerformanceProfileReport {
    let minimum_memory_bytes = (variant.requirements.min_ram_gb * GIB).ceil() as u64;
    let has_capacity = hardware.total_ram_bytes >= minimum_memory_bytes;
    let has_available_memory = hardware.available_ram_bytes >= minimum_memory_bytes;
    let fits_detected_memory =
        has_capacity && (profile == PerformanceProfile::Custom || has_available_memory);
    let accelerator = hardware
        .accelerators
        .first()
        .map(|device| accelerator_backend(device.kind).to_owned())
        .unwrap_or_else(|| "cpu".to_owned());
    let detected_vram_gib = hardware
        .accelerators
        .iter()
        .filter_map(|device| device.total_vram_bytes)
        .sum::<u64>() as f64
        / GIB;
    let required_vram_gib = variant.requirements.min_vram_gb.unwrap_or_default();
    let vram_shortfall_gib = (required_vram_gib - detected_vram_gib).max(0.0);
    let detected_gpu_count = hardware
        .accelerators
        .iter()
        .filter(|device| {
            device.kind == lumen_source_hardware::AcceleratorKind::Nvidia
                && device.total_vram_bytes.is_some()
        })
        .count()
        .clamp(1, usize::from(u16::MAX)) as u16;
    let maximum_context = variant.context_window_tokens;
    let (context_limit, concurrent_requests, gpu_memory_utilization, summary) = match profile {
        PerformanceProfile::Safe => (
            2_048,
            1,
            0.65,
            "Uses a smaller context and one request at a time to leave more memory available.",
        ),
        PerformanceProfile::Balanced => (
            4_096,
            2,
            0.80,
            "Balances response quality, speed, and memory use for this machine.",
        ),
        PerformanceProfile::Fast => (
            if hardware.total_ram_bytes >= minimum_memory_bytes.saturating_mul(2) {
                16_384
            } else {
                8_192
            },
            4,
            0.90,
            "Uses more memory and concurrency to prioritize throughput.",
        ),
        PerformanceProfile::Custom => (
            0,
            0,
            0.0,
            "Keeps runtime defaults so you can tune the detailed model settings yourself.",
        ),
    };
    let context_length = if profile == PerformanceProfile::Custom {
        None
    } else {
        Some(maximum_context.map_or(context_limit, |maximum| maximum.min(context_limit)))
    };
    let mut settings = ModelSettings {
        performance_profile: Some(profile),
        context_length,
        preferred_accelerator: (profile != PerformanceProfile::Custom).then(|| accelerator.clone()),
        ..ModelSettings::default()
    };
    if variant.runtime == VLLM_RUNTIME && profile != PerformanceProfile::Custom {
        settings.vllm_gpu_memory_utilization = Some(gpu_memory_utilization);
        settings.vllm_max_concurrent_sequences = Some(concurrent_requests);
        settings.vllm_prefix_caching = Some(profile != PerformanceProfile::Safe);
        settings.vllm_cpu_offload_gib = Some(match profile {
            PerformanceProfile::Safe => vram_shortfall_gib.ceil() as f32,
            PerformanceProfile::Balanced => vram_shortfall_gib as f32,
            PerformanceProfile::Fast | PerformanceProfile::Custom => 0.0,
        });
        settings.vllm_tensor_parallel_size = Some(detected_gpu_count);
        settings.vllm_pipeline_parallel_size = Some(1);
    }

    let mut warnings = Vec::new();
    if !has_capacity {
        warnings.push(format!(
            "This model needs at least {:.1} GiB of system memory, but the machine has {:.1} GiB.",
            variant.requirements.min_ram_gb,
            hardware.total_ram_bytes as f64 / GIB
        ));
    } else if !has_available_memory {
        let action = if profile == PerformanceProfile::Custom {
            "Custom is proceeding explicitly; close other applications before starting the model."
        } else {
            "Close other applications or choose Custom to proceed with an explicit warning."
        };
        warnings.push(format!(
            "This model needs about {:.1} GiB free, but only {:.1} GiB is currently available. {action}",
            variant.requirements.min_ram_gb,
            hardware.available_ram_bytes as f64 / GIB
        ));
    } else if profile == PerformanceProfile::Fast
        && hardware.available_ram_bytes < minimum_memory_bytes.saturating_mul(3) / 2
    {
        warnings.push(
            "Fast may compete with other applications for memory. Balanced is the safer choice."
                .to_owned(),
        );
    }
    if accelerator == "cpu" && profile == PerformanceProfile::Fast {
        warnings.push(
            "No supported GPU was detected, so Fast may not improve response speed.".to_owned(),
        );
    }

    PerformanceProfileReport {
        profile,
        settings,
        summary: summary.to_owned(),
        accelerator: if profile == PerformanceProfile::Custom {
            "runtime default".to_owned()
        } else {
            accelerator
        },
        context_length,
        concurrent_requests,
        minimum_memory_bytes,
        available_memory_bytes: hardware.available_ram_bytes,
        fits_detected_memory,
        warnings,
        hardware_summary: hardware_summary(hardware),
    }
}

fn hardware_summary(hardware: &HardwareFacts) -> String {
    let accelerator = hardware
        .accelerators
        .first()
        .map(|device| device.name.as_str())
        .unwrap_or("CPU only");
    format!(
        "{} {} · {:.1} GiB RAM · {accelerator}",
        hardware.os.family,
        hardware.os.architecture,
        hardware.total_ram_bytes as f64 / GIB
    )
}

fn validation_check(id: &str, status: &str, detail: &str) -> InstallationValidationCheck {
    InstallationValidationCheck {
        id: id.to_owned(),
        status: status.to_owned(),
        detail: detail.to_owned(),
    }
}

fn failed_validation_report(
    variant: &ModelVariant,
    capability: String,
    settings: ModelSettings,
    checks: Vec<InstallationValidationCheck>,
    message: &str,
) -> InstallationValidationReport {
    InstallationValidationReport {
        passed: false,
        capability,
        runtime_id: variant.runtime.clone(),
        runtime_model_id: variant.runtime_ref.clone(),
        message: message.to_owned(),
        accelerator: None,
        hardware_summary: None,
        effective_context_length: None,
        validated_at: chrono::Utc::now().to_rfc3339(),
        running: false,
        settings,
        checks,
    }
}

fn ensure_supported_runtime(variant: &ModelVariant) -> Result<(), String> {
    let registry = RuntimeRegistry::default();
    if registry.supports(&variant.runtime)
        && registry
            .resolve_name(&variant.runtime)
            .is_some_and(|runtime| {
                runtime.capabilities.artifact_acquisition || runtime.id == RuntimeId::Dummy
            })
    {
        Ok(())
    } else {
        Err(format!(
            "Runtime `{}` is not supported by this version of Lumen Source",
            variant.runtime
        ))
    }
}

fn supports_catalog_runtime(registry: &RuntimeRegistry, runtime_id: &str, target_id: &str) -> bool {
    registry.resolve_name(runtime_id).is_some_and(|runtime| {
        let locally_installable =
            runtime.capabilities.artifact_acquisition || runtime.id == RuntimeId::Dummy;
        locally_installable && (target_id == "local" || runtime.capabilities.remote_connection)
    })
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

fn estimated_loaded_memory(variant: &ModelVariant) -> (u64, u64) {
    let minimum = (variant.requirements.min_ram_gb * GIB).ceil() as u64;
    let context_reserve = variant
        .context_window_tokens
        .map_or(512 * 1024 * 1024, |tokens| {
            u64::from(tokens.min(32_768)).saturating_mul(64 * 1024)
        });
    (minimum, minimum.saturating_add(context_reserve))
}

fn recommendation_labels(
    model: &ModelEntry,
    variant: &ModelVariant,
    compatible: bool,
) -> Vec<String> {
    let mut labels = Vec::new();
    if compatible && variant.requirements.min_ram_gb <= 16.0 && variant.parameters_b <= 8.0 {
        labels.push("Beginner friendly".to_owned());
    }
    if variant.parameters_b <= 8.0 {
        labels.push("Fast".to_owned());
    } else if variant.parameters_b >= 14.0 {
        labels.push("High quality".to_owned());
    }
    if model
        .capabilities
        .iter()
        .any(|capability| capability.contains("reason"))
    {
        labels.push("Reasoning".to_owned());
    }
    if model
        .capabilities
        .iter()
        .any(|capability| capability == "embeddings")
    {
        labels.push("Embeddings".to_owned());
    }
    if model
        .capabilities
        .iter()
        .any(|capability| capability.contains("vision") || capability.contains("image"))
    {
        labels.push("Vision".to_owned());
    }
    labels
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
    use lumen_source_hardware::{
        AcceleratorFacts, AcceleratorKind, CpuFacts, MemoryFacts, OsFacts, StorageFacts,
    };
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
        assert_eq!(state.models[0].runtime_id, OLLAMA_RUNTIME);
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

    #[test]
    fn performance_profiles_are_hardware_aware_and_persist_their_identity() {
        let Ok(catalog) = Catalog::from_slice(BUNDLED_CATALOG) else {
            panic!("bundled model list should parse");
        };
        let Some(variant) = catalog
            .models
            .iter()
            .flat_map(|model| &model.variants)
            .next()
        else {
            panic!("bundled catalog should contain a model variant");
        };
        let gib = 1024_u64.pow(3);
        let hardware = HardwareFacts {
            os: OsFacts {
                family: "windows".to_owned(),
                distribution: None,
                version: None,
                architecture: "x86_64".to_owned(),
            },
            cpu: CpuFacts {
                model: Some("Test CPU".to_owned()),
                architecture: "x86_64".to_owned(),
                logical_cores: 16,
                physical_cores: Some(8),
                frequency_mhz: None,
            },
            memory: MemoryFacts::default(),
            total_ram_bytes: 32 * gib,
            available_ram_bytes: 24 * gib,
            storage: StorageFacts {
                mount_point: PathBuf::from("C:\\"),
                total_bytes: 512 * gib,
                available_bytes: 256 * gib,
            },
            accelerators: vec![AcceleratorFacts {
                kind: AcceleratorKind::Nvidia,
                name: "Test GPU".to_owned(),
                total_vram_bytes: Some(12 * gib),
                driver_version: None,
            }],
        };

        let safe = build_performance_profile_report(variant, &hardware, PerformanceProfile::Safe);
        let balanced =
            build_performance_profile_report(variant, &hardware, PerformanceProfile::Balanced);

        assert_eq!(
            safe.settings.performance_profile,
            Some(PerformanceProfile::Safe)
        );
        assert_eq!(
            balanced.settings.performance_profile,
            Some(PerformanceProfile::Balanced)
        );
        assert_eq!(balanced.accelerator, "cuda");
        assert!(safe.context_length <= balanced.context_length);
        assert!(safe.concurrent_requests < balanced.concurrent_requests);
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
            runtime_id: OLLAMA_RUNTIME.to_owned(),
            runtime_model_id: Some("qwen2.5-coder:14b".to_owned()),
            runtime_capabilities: crate::runtime_registry::capabilities_for(OLLAMA_RUNTIME),
            model_settings: None,
            installation_validation: None,
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
            runtime_id: DUMMY_RUNTIME.to_owned(),
            runtime_model_id: Some("dummy-test-model:latest".to_owned()),
            runtime_capabilities: crate::runtime_registry::capabilities_for(DUMMY_RUNTIME),
            model_settings: None,
            installation_validation: None,
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
            runtime_id: DUMMY_RUNTIME.to_owned(),
            runtime_model_id: Some("dummy-test-model:latest".to_owned()),
            runtime_capabilities: crate::runtime_registry::capabilities_for(DUMMY_RUNTIME),
            model_settings: None,
            installation_validation: None,
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
            runtime_id: OLLAMA_RUNTIME.to_owned(),
            runtime_model_id: Some("qwen2.5-coder:14b".to_owned()),
            runtime_capabilities: crate::runtime_registry::capabilities_for(OLLAMA_RUNTIME),
            model_settings: None,
            installation_validation: None,
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
