//! Thin adaptation seam between Tauri and the shared Lumen Source crates.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use lumen_source_catalog::{
    Catalog, CatalogCache, CatalogLocation, CatalogService, CatalogSource, Ed25519Verifier,
    License, ModelEntry, ModelVariant, Platform, ReqwestCatalogFetcher,
};
use lumen_source_hardware::{AcceleratorKind, HardwareFacts, HardwareProbe, PlatformHardwareProbe};
use lumen_source_host::{Host, LocalHost};
use lumen_source_recommend::{recommend, RecommendationRequest};
use lumen_source_runtime::{
    Artifact as RuntimeArtifact, ArtifactInstaller, CancellationToken, ChatMessage, ChatProgress,
    DummyRuntime, InstalledModel, OllamaRuntime, Runtime, RuntimeEndpoint, RuntimeError,
    RuntimeProgress, RuntimeStatus as CoreRuntimeStatus, Url,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};
use zeroize::Zeroizing;

use crate::remote::{
    connect as connect_remote, RemoteAuthentication, RemoteConnectionReport, RemoteSession,
    RemoteTargetConfig, RemoteTargetProfile,
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
const DUMMY_RUNTIME: &str = "dummy-runtime";

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
    remote_targets: Vec<RemoteTargetConfig>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedModelEntry {
    pub id: String,
    pub name: String,
    pub model_id: String,
    pub model_name: String,
    #[serde(default)]
    pub runtime_model_id: Option<String>,
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

fn default_managed() -> bool {
    true
}

fn local_target_id() -> String {
    "local".to_owned()
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

impl SharedCoreAdapter {
    pub fn new() -> Result<Self, String> {
        let data_root = dirs::data_local_dir()
            .ok_or_else(|| "No local application data directory is available".to_owned())?;
        let state_path = data_root.join("lumen-source/state.json");
        let telemetry = Telemetry::new(&data_root);
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
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("ollama")),
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
        if target_id == "local" {
            return Ok(Arc::clone(&self.runtime));
        }
        if let Some(session) = self.remote_session.read().await.clone() {
            if session.target_id() == target_id && session.healthy().await {
                return Ok(Arc::clone(&session.runtime));
            }
        }
        let config = self
            .state
            .read()
            .await
            .remote_targets
            .iter()
            .find(|target| target.target_id() == target_id)
            .cloned()
            .ok_or_else(|| format!("Remote target `{target_id}` is not configured"))?;
        if config.authentication == RemoteAuthentication::Password {
            return Err(
                "The password-authenticated SSH session is disconnected. Reconnect this target from Add model and enter its password again; LumenSource does not save SSH passwords."
                    .to_owned(),
            );
        }
        let attempt = connect_remote(config, None).await?;
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
        self.runtime_for_target(target_id).await?;
        self.remote_session
            .read()
            .await
            .as_ref()
            .filter(|session| session.target_id() == target_id)
            .map(|session| session.hardware.clone())
            .ok_or_else(|| "The remote target hardware profile is unavailable".to_owned())
    }

    pub async fn detect_hardware(&self, target_id: &str) -> Result<HardwareProfile, String> {
        let facts = self.hardware_for_target(target_id).await?;
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
                    check(
                        "connection",
                        "Remote Linux connection",
                        "pass",
                        "The SSH tunnel to the remote Linux target is available.",
                    ),
                    check(
                        "hardware",
                        "Remote hardware compatibility",
                        if hardware_ok { "pass" } else { "fail" },
                        &hardware_detail,
                    ),
                    check(
                        "storage",
                        "Remote available storage",
                        if storage_ok { "pass" } else { "fail" },
                        if storage_ok {
                            "Enough free space is available on the target filesystem."
                        } else {
                            "The model requires more free storage on the target filesystem."
                        },
                    ),
                    check(
                        "runtime",
                        "Remote Ollama service",
                        "pass",
                        "Ollama is installed and its loopback API is reachable on the target.",
                    ),
                    check(
                        "source",
                        "Model source",
                        "pass",
                        &format!(
                            "The remote Ollama service will pull the catalog-pinned model reference `{}`.",
                            variant.runtime_ref
                        ),
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
        let (runtime_label, runtime_detail) = if is_dummy {
            (
                "Dummy test runtime",
                "The in-memory test runtime performs no downloads and launches no server."
                    .to_owned(),
            )
        } else if runtime_available {
            (
                "Ollama runtime",
                "Ollama is installed or already running.".to_owned(),
            )
        } else if let Some(Ok((artifact, _))) = &runtime_artifact {
            (
                "Ollama runtime",
                format!(
                    "Download {} and verify SHA-256 {}.",
                    artifact.url, artifact.sha256
                ),
            )
        } else {
            (
                "Ollama runtime",
                "No compatible verified runtime artifact is available.".to_owned(),
            )
        };
        let storage_ok = available >= required;
        Ok(PreflightReport {
            can_install: hardware_ok && storage_ok && (runtime_available || runtime_installable),
            required_bytes: required,
            available_bytes: available,
            checks: vec![
                check(
                    "hardware",
                    "Hardware compatibility",
                    if hardware_ok { "pass" } else { "fail" },
                    &hardware_detail,
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
                    runtime_label,
                    if runtime_available {
                        "pass"
                    } else if runtime_installable {
                        "warning"
                    } else {
                        "fail"
                    },
                    &runtime_detail,
                ),
                check(
                    "source",
                    "Model source",
                    "pass",
                    &if is_dummy {
                        format!(
                            "The catalog test model `{}` will be registered in memory.",
                            variant.runtime_ref
                        )
                    } else {
                        format!(
                            "Ollama will pull the catalog-pinned model reference `{}`.",
                            variant.runtime_ref
                        )
                    },
                ),
            ],
        })
    }

    pub async fn install(
        &self,
        app: AppHandle,
        variant_id: String,
        target_id: String,
        license_basis: String,
        license_reference: Option<String>,
        license_acknowledged: bool,
    ) -> Result<(), String> {
        self.validate_license_authorization(
            &variant_id,
            &license_basis,
            license_reference.as_deref(),
            license_acknowledged,
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
            .install_inner(app, variant_id, &target_id, &cancellation)
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
                    "Preparing dummy test runtime…"
                } else if target_id != "local" {
                    "Preparing the remote Ollama pull…"
                } else {
                    "Starting Ollama…"
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
                let (artifact, version) = runtime_artifact(&catalog, &variant.runtime)?;
                let data_root = dirs::data_local_dir()
                    .ok_or_else(|| "No local application data directory is available".to_owned())?;
                let install_dir = data_root
                    .join("lumen-source/runtimes")
                    .join(&variant.runtime)
                    .join(version);
                let executable = ArtifactInstaller::default()
                    .install_tar_zst_cancellable(
                        &artifact,
                        &install_dir,
                        std::path::Path::new("bin/ollama"),
                        &reporter,
                        cancellation,
                    )
                    .await
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
                "Installation complete",
            ),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub async fn start(
        &self,
        variant_id: String,
        target_id: String,
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
                let runtime = self.runtime_for_target(&target_id).await?;
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
            let runtime = self.runtime_for_target(&target_id).await?;
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
            if !self.runtime.executable_available().await {
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

fn accelerator_backend(kind: AcceleratorKind) -> &'static str {
    match kind {
        AcceleratorKind::Nvidia => "cuda",
        AcceleratorKind::Amd => "rocm",
        AcceleratorKind::Intel => "intel",
        AcceleratorKind::Other => "other",
    }
}

fn deployment_kind(target_id: &str) -> String {
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
pub struct PerformanceSnapshot {
    pub model_id: String,
    pub state: String,
    pub sampled_at_unix_ms: u64,
    pub allocated_memory_bytes: u64,
    pub allocated_vram_bytes: u64,
    pub allocated_system_memory_bytes: u64,
    pub context_length: Option<u64>,
}

fn current_unix_time_ms() -> u64 {
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
    pub fit: String,
    pub reasons: Vec<String>,
    pub recommended: bool,
    pub compatible: bool,
    pub license: LicenseSummary,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_item: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_items: Option<u32>,
    pub message: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum ChatEvent {
    Delta { content: String },
    Done,
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
    pub completions_url: String,
    pub embeddings_url: String,
    pub model: String,
    pub api_key_required: bool,
    pub api_available: bool,
    pub chat_available: bool,
    pub embeddings_available: bool,
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
    current_item: Option<u32>,
    total_items: Option<u32>,
    message: &str,
) -> InstallProgress {
    InstallProgress {
        model_id: model_id.to_owned(),
        phase: phase.to_owned(),
        completed_bytes: completed,
        total_bytes: total,
        current_item,
        total_items,
        message: message.to_owned(),
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
            "Downloading runtime…",
        ),
        RuntimeProgress::Verifying => progress(
            model_id,
            "verifying",
            fallback_total,
            fallback_total,
            None,
            None,
            "Verifying download…",
        ),
        RuntimeProgress::Installing => progress(
            model_id,
            "installing",
            fallback_total,
            fallback_total,
            None,
            None,
            "Installing runtime…",
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
            progress(
                model_id,
                "downloading",
                completed.unwrap_or_default(),
                total.unwrap_or(fallback_total),
                current_item,
                total_items,
                &status,
            )
        }
        RuntimeProgress::Ready => progress(
            model_id,
            "installing",
            fallback_total,
            fallback_total,
            None,
            None,
            "Registering model…",
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

fn reconcile_unavailable_models(
    catalog: &Catalog,
    mut persisted: Vec<PersistedModelEntry>,
    dummy_installed: &[InstalledModel],
    dummy_running: &[String],
) -> Vec<PersistedModelEntry> {
    for entry in &mut persisted {
        let dummy_reference = catalog.models.iter().find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| variant.id == entry.model_id && variant.runtime == DUMMY_RUNTIME)
                .map(|variant| variant.runtime_ref.as_str())
        });
        entry.running = dummy_reference
            .is_some_and(|reference| dummy_running.iter().any(|running| running == reference));
    }
    upsert_dummy_models(catalog, &mut persisted, dummy_installed, dummy_running);
    sort_models(&mut persisted);
    persisted
}

fn reconcile_models(
    catalog: Catalog,
    mut persisted: Vec<PersistedModelEntry>,
    installed: Vec<InstalledModel>,
    dummy_installed: &[InstalledModel],
    running: &[String],
) -> Vec<PersistedModelEntry> {
    let mut result = Vec::with_capacity(installed.len());
    for installed_model in installed {
        let catalog_match = catalog.models.iter().find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| {
                    variant.runtime == "ollama"
                        && same_ollama_reference(&variant.runtime_ref, &installed_model.name)
                })
                .map(|variant| (model, variant))
        });
        let mut previous_entries = Vec::new();
        let mut remaining = Vec::with_capacity(persisted.len());
        for entry in persisted {
            let matches_installed = entry
                .runtime_model_id
                .as_deref()
                .is_some_and(|name| same_ollama_reference(name, &installed_model.name))
                || catalog_match.is_some_and(|(_, variant)| entry.model_id == variant.id);
            if matches_installed {
                previous_entries.push(entry);
            } else {
                remaining.push(entry);
            }
        }
        persisted = remaining;
        let previous_entries = if previous_entries.is_empty() {
            vec![None]
        } else {
            previous_entries.into_iter().map(Some).collect()
        };
        let is_running = running
            .iter()
            .any(|name| same_ollama_reference(name, &installed_model.name));

        for previous in previous_entries {
            let entry = if let Some((model, variant)) = catalog_match {
                let runtime_version = catalog
                    .runtimes
                    .iter()
                    .find(|runtime| runtime.id == variant.runtime)
                    .map(|runtime| runtime.install.version.clone())
                    .unwrap_or_else(|| "unknown".to_owned());
                PersistedModelEntry {
                    id: previous
                        .as_ref()
                        .map(|entry| entry.id.clone())
                        .unwrap_or_else(|| discovered_id(&installed_model)),
                    name: previous
                        .as_ref()
                        .map(|entry| entry.name.clone())
                        .unwrap_or_else(|| model.display_name.clone()),
                    model_id: variant.id.clone(),
                    model_name: model.display_name.clone(),
                    runtime_model_id: Some(installed_model.name.clone()),
                    version: runtime_version,
                    location: "local".to_owned(),
                    target_id: local_target_id(),
                    target_name: None,
                    running: is_running,
                    managed: true,
                    digest: installed_model.digest.clone(),
                    size_bytes: installed_model.size_bytes,
                    license_basis: previous
                        .as_ref()
                        .and_then(|entry| entry.license_basis.clone()),
                    license_reference: previous
                        .as_ref()
                        .and_then(|entry| entry.license_reference.clone()),
                    license_acknowledged_at: previous
                        .as_ref()
                        .and_then(|entry| entry.license_acknowledged_at.clone()),
                    license_profile_id: previous
                        .as_ref()
                        .and_then(|entry| entry.license_profile_id.clone()),
                    license_name: previous
                        .as_ref()
                        .and_then(|entry| entry.license_name.clone()),
                    license_url: previous
                        .as_ref()
                        .and_then(|entry| entry.license_url.clone()),
                    license_reviewed_at: previous
                        .as_ref()
                        .and_then(|entry| entry.license_reviewed_at.clone()),
                    license_catalog_version: previous
                        .as_ref()
                        .and_then(|entry| entry.license_catalog_version.clone()),
                    logs: previous.as_ref().map_or_else(
                        || vec!["Discovered in the local Ollama model store.".to_owned()],
                        |entry| entry.logs.clone(),
                    ),
                }
            } else {
                PersistedModelEntry {
                    id: previous
                        .as_ref()
                        .map(|entry| entry.id.clone())
                        .unwrap_or_else(|| discovered_id(&installed_model)),
                    name: previous
                        .as_ref()
                        .map(|entry| entry.name.clone())
                        .unwrap_or_else(|| installed_model.name.clone()),
                    model_id: format!("external:{}", installed_model.name),
                    model_name: installed_model.name.clone(),
                    runtime_model_id: Some(installed_model.name.clone()),
                    version: "External Ollama model".to_owned(),
                    location: "local".to_owned(),
                    target_id: local_target_id(),
                    target_name: None,
                    running: is_running,
                    managed: false,
                    digest: installed_model.digest.clone(),
                    size_bytes: installed_model.size_bytes,
                    license_basis: previous
                        .as_ref()
                        .and_then(|entry| entry.license_basis.clone()),
                    license_reference: previous
                        .as_ref()
                        .and_then(|entry| entry.license_reference.clone()),
                    license_acknowledged_at: previous
                        .as_ref()
                        .and_then(|entry| entry.license_acknowledged_at.clone()),
                    license_profile_id: previous
                        .as_ref()
                        .and_then(|entry| entry.license_profile_id.clone()),
                    license_name: previous
                        .as_ref()
                        .and_then(|entry| entry.license_name.clone()),
                    license_url: previous
                        .as_ref()
                        .and_then(|entry| entry.license_url.clone()),
                    license_reviewed_at: previous
                        .as_ref()
                        .and_then(|entry| entry.license_reviewed_at.clone()),
                    license_catalog_version: previous
                        .as_ref()
                        .and_then(|entry| entry.license_catalog_version.clone()),
                    logs: previous.as_ref().map_or_else(
                        || vec!["Discovered outside the active Lumen Source catalog.".to_owned()],
                        |entry| entry.logs.clone(),
                    ),
                }
            };
            result.push(entry);
        }
    }
    for mut entry in persisted {
        let dummy_variant = catalog.models.iter().find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| variant.id == entry.model_id && variant.runtime == DUMMY_RUNTIME)
        });
        if let Some(variant) = dummy_variant {
            entry.running = running.iter().any(|model| model == &variant.runtime_ref);
            result.push(entry);
        }
    }
    upsert_dummy_models(&catalog, &mut result, dummy_installed, running);
    sort_models(&mut result);
    result
}

fn upsert_dummy_models(
    catalog: &Catalog,
    models: &mut Vec<PersistedModelEntry>,
    installed: &[InstalledModel],
    running: &[String],
) {
    for installed_model in installed {
        let Some((model, variant)) = catalog.models.iter().find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| {
                    variant.runtime == DUMMY_RUNTIME && variant.runtime_ref == installed_model.name
                })
                .map(|variant| (model, variant))
        }) else {
            continue;
        };
        let is_running = running.iter().any(|name| name == &installed_model.name);
        let mut found = false;
        for entry in models.iter_mut().filter(|entry| {
            entry.model_id == variant.id
                || entry.runtime_model_id.as_deref() == Some(installed_model.name.as_str())
        }) {
            found = true;
            entry.running = is_running;
            entry.digest = installed_model.digest.clone();
            entry.size_bytes = installed_model.size_bytes;
        }
        if found {
            continue;
        }
        let version = catalog
            .runtimes
            .iter()
            .find(|runtime| runtime.id == variant.runtime)
            .map(|runtime| runtime.install.version.clone())
            .unwrap_or_else(|| "unknown".to_owned());
        models.push(PersistedModelEntry {
            id: format!("dummy:{}", installed_model.name),
            name: model.display_name.clone(),
            model_id: variant.id.clone(),
            model_name: model.display_name.clone(),
            runtime_model_id: Some(installed_model.name.clone()),
            version,
            location: "local".to_owned(),
            target_id: local_target_id(),
            target_name: None,
            running: is_running,
            managed: true,
            digest: installed_model.digest.clone(),
            size_bytes: installed_model.size_bytes,
            license_basis: None,
            license_reference: None,
            license_acknowledged_at: None,
            license_profile_id: None,
            license_name: None,
            license_url: None,
            license_reviewed_at: None,
            license_catalog_version: None,
            logs: vec!["Discovered in the dummy test runtime.".to_owned()],
        });
    }
}

fn with_remote_models(
    mut local_models: Vec<PersistedModelEntry>,
    remote_models: Vec<PersistedModelEntry>,
) -> Vec<PersistedModelEntry> {
    local_models.extend(remote_models);
    sort_models(&mut local_models);
    local_models
}

fn sort_models(models: &mut [PersistedModelEntry]) {
    models.sort_by(|left, right| {
        right
            .running
            .cmp(&left.running)
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn same_ollama_reference(left: &str, right: &str) -> bool {
    normalize_ollama_reference(left) == normalize_ollama_reference(right)
}

fn normalize_ollama_reference(reference: &str) -> String {
    let reference = reference.trim();
    let last_slash = reference.rfind('/').map_or(0, |index| index + 1);
    if reference[last_slash..].contains(':') {
        reference.to_owned()
    } else {
        format!("{reference}:latest")
    }
}

fn discovered_id(model: &InstalledModel) -> String {
    format!(
        "ollama:{}",
        model.digest.as_deref().unwrap_or(model.name.as_str())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
