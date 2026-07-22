//! UI-independent application state and orchestration services.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use lumen_source_host::{
    HardwareFacts, Host, HostError, HostStatus, RuntimeInstall, UsageSnapshot,
};
use lumen_source_runtime::{RuntimeEndpoint, RuntimeProgress};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error(transparent)]
    Host(#[from] HostError),
    #[error("catalog operation failed: {0}")]
    Catalog(String),
    #[error("recommendation failed: {0}")]
    Recommendation(String),
    #[error("operation is invalid while application is {0:?}")]
    InvalidState(OrchestrationState),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: String,
    pub minimum_memory_bytes: u64,
    pub recommended_memory_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recommendation {
    pub model_id: String,
    pub reason: String,
}

#[async_trait]
pub trait Catalog: Send + Sync {
    async fn models(&self) -> Result<Vec<CatalogModel>, ApplicationError>;
}

pub trait RecommendationEngine: Send + Sync {
    fn recommend(
        &self,
        hardware: &HardwareFacts,
        models: &[CatalogModel],
    ) -> Result<Recommendation, ApplicationError>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallationState {
    #[default]
    NotInstalled,
    InstallingRuntime,
    InstallingModel {
        model_id: String,
    },
    Installed {
        executable: PathBuf,
        model_id: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrchestrationState {
    #[default]
    Idle,
    InspectingHardware,
    Recommending,
    Installing,
    Starting,
    Running {
        model_id: String,
    },
    Stopping,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressEvent {
    StateChanged(OrchestrationState),
    InstallationChanged(InstallationState),
    Runtime(RuntimeProgress),
    RecommendationReady(Recommendation),
    EndpointReady(RuntimeEndpoint),
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);
}

impl<F> EventSink for F
where
    F: Fn(ProgressEvent) + Send + Sync,
{
    fn emit(&self, event: ProgressEvent) {
        self(event);
    }
}

#[derive(Clone, Debug, Default)]
struct ServiceState {
    orchestration: OrchestrationState,
    installation: InstallationState,
}

/// Coordinates catalog, recommendation, and host adapters without UI concerns.
pub struct LumenService<H, C, R> {
    host: Arc<H>,
    catalog: Arc<C>,
    recommendation: Arc<R>,
    events: Arc<dyn EventSink>,
    state: Mutex<ServiceState>,
}

impl<H, C, R> LumenService<H, C, R>
where
    H: Host,
    C: Catalog,
    R: RecommendationEngine,
{
    pub fn new(
        host: Arc<H>,
        catalog: Arc<C>,
        recommendation: Arc<R>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            host,
            catalog,
            recommendation,
            events,
            state: Mutex::new(ServiceState::default()),
        }
    }

    pub async fn orchestration_state(&self) -> OrchestrationState {
        self.state.lock().await.orchestration.clone()
    }

    pub async fn installation_state(&self) -> InstallationState {
        self.state.lock().await.installation.clone()
    }

    pub async fn inspect_hardware(
        &self,
    ) -> Result<(HardwareFacts, UsageSnapshot), ApplicationError> {
        self.set_orchestration(OrchestrationState::InspectingHardware)
            .await;
        let result: Result<(HardwareFacts, UsageSnapshot), ApplicationError> = async {
            let facts = self.host.hardware_facts().await?;
            let usage = self.host.hardware_usage().await?;
            Ok((facts, usage))
        }
        .await;
        self.finish_operation(&result).await;
        result
    }

    pub async fn recommend(&self) -> Result<Recommendation, ApplicationError> {
        self.set_orchestration(OrchestrationState::Recommending)
            .await;
        let result: Result<Recommendation, ApplicationError> = async {
            let hardware = self.host.hardware_facts().await?;
            let models = self.catalog.models().await?;
            self.recommendation.recommend(&hardware, &models)
        }
        .await;
        match &result {
            Ok(recommendation) => {
                self.events
                    .emit(ProgressEvent::RecommendationReady(recommendation.clone()));
                self.set_orchestration(OrchestrationState::Idle).await;
            }
            Err(_) => self.set_orchestration(OrchestrationState::Failed).await,
        }
        result
    }

    pub async fn install(
        &self,
        install: &RuntimeInstall,
        model_id: &str,
    ) -> Result<PathBuf, ApplicationError> {
        self.require_idle().await?;
        self.set_orchestration(OrchestrationState::Installing).await;
        self.set_installation(InstallationState::InstallingRuntime)
            .await;
        let runtime_reporter = |progress| {
            self.events.emit(ProgressEvent::Runtime(progress));
        };
        let result: Result<PathBuf, ApplicationError> = async {
            let executable = self
                .host
                .install_runtime(install, &runtime_reporter)
                .await?;
            self.set_installation(InstallationState::InstallingModel {
                model_id: model_id.to_owned(),
            })
            .await;
            self.host.install_model(model_id, &runtime_reporter).await?;
            Ok(executable)
        }
        .await;
        match &result {
            Ok(executable) => {
                self.set_installation(InstallationState::Installed {
                    executable: executable.clone(),
                    model_id: model_id.to_owned(),
                })
                .await;
                self.set_orchestration(OrchestrationState::Idle).await;
            }
            Err(error) => {
                self.set_installation(InstallationState::Failed {
                    message: error.to_string(),
                })
                .await;
                self.set_orchestration(OrchestrationState::Failed).await;
            }
        }
        result
    }

    pub async fn start(&self, model_id: &str) -> Result<RuntimeEndpoint, ApplicationError> {
        self.require_idle().await?;
        self.set_orchestration(OrchestrationState::Starting).await;
        let result = self
            .host
            .start(model_id)
            .await
            .map_err(ApplicationError::from);
        match &result {
            Ok(endpoint) => {
                self.events
                    .emit(ProgressEvent::EndpointReady(endpoint.clone()));
                self.set_orchestration(OrchestrationState::Running {
                    model_id: model_id.to_owned(),
                })
                .await;
            }
            Err(_) => self.set_orchestration(OrchestrationState::Failed).await,
        }
        result
    }

    pub async fn stop(&self) -> Result<(), ApplicationError> {
        let model_id = match self.orchestration_state().await {
            OrchestrationState::Running { model_id } => model_id,
            state => return Err(ApplicationError::InvalidState(state)),
        };
        self.set_orchestration(OrchestrationState::Stopping).await;
        let result = self
            .host
            .stop(&model_id)
            .await
            .map_err(ApplicationError::from);
        self.finish_operation(&result).await;
        result
    }

    pub async fn status(&self) -> Result<HostStatus, ApplicationError> {
        Ok(self.host.status().await?)
    }

    async fn require_idle(&self) -> Result<(), ApplicationError> {
        let current = self.orchestration_state().await;
        if current == OrchestrationState::Idle {
            Ok(())
        } else {
            Err(ApplicationError::InvalidState(current))
        }
    }

    async fn set_orchestration(&self, orchestration: OrchestrationState) {
        self.state.lock().await.orchestration = orchestration.clone();
        self.events.emit(ProgressEvent::StateChanged(orchestration));
    }

    async fn set_installation(&self, installation: InstallationState) {
        self.state.lock().await.installation = installation.clone();
        self.events
            .emit(ProgressEvent::InstallationChanged(installation));
    }

    async fn finish_operation<T>(&self, result: &Result<T, ApplicationError>) {
        let state = if result.is_ok() {
            OrchestrationState::Idle
        } else {
            OrchestrationState::Failed
        };
        self.set_orchestration(state).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use lumen_source_hardware::{CpuFacts, MemoryFacts, OsFacts, ProbeError, StorageFacts};
    use lumen_source_host::HostStatus;
    use lumen_source_runtime::{InstalledModel, ProgressReporter, RuntimeStatus, Url};
    use std::sync::Mutex as StdMutex;

    struct FakeHost {
        endpoint: RuntimeEndpoint,
    }

    #[async_trait]
    impl Host for FakeHost {
        async fn hardware_facts(&self) -> Result<HardwareFacts, HostError> {
            Ok(HardwareFacts {
                os: OsFacts {
                    family: "linux".to_owned(),
                    distribution: Some("ubuntu".to_owned()),
                    version: Some("24.04".to_owned()),
                    architecture: "x86_64".to_owned(),
                },
                cpu: CpuFacts {
                    model: Some("Test CPU".to_owned()),
                    architecture: "x86_64".to_owned(),
                    logical_cores: 4,
                    physical_cores: Some(2),
                    frequency_mhz: Some(3_200),
                },
                memory: MemoryFacts {
                    kind: Some("DDR4".to_owned()),
                    speed_mts: Some(3_200),
                },
                total_ram_bytes: 8,
                available_ram_bytes: 6,
                storage: StorageFacts {
                    mount_point: PathBuf::from("/"),
                    total_bytes: 20,
                    available_bytes: 10,
                },
                accelerators: Vec::new(),
            })
        }

        async fn hardware_usage(&self) -> Result<UsageSnapshot, HostError> {
            Ok(UsageSnapshot {
                sampled_at_unix_ms: 0,
                cpu_utilization_percent: 1.0,
                used_ram_bytes: 2,
                available_ram_bytes: 6,
                accelerators: Vec::new(),
            })
        }

        async fn install_runtime(
            &self,
            _install: &RuntimeInstall,
            _progress: &dyn ProgressReporter,
        ) -> Result<PathBuf, HostError> {
            Ok(PathBuf::from("/fixed/ollama"))
        }

        async fn install_model(
            &self,
            _model: &str,
            progress: &dyn ProgressReporter,
        ) -> Result<(), HostError> {
            progress.report(RuntimeProgress::Ready);
            Ok(())
        }

        async fn installed_models(&self) -> Result<Vec<InstalledModel>, HostError> {
            Ok(Vec::new())
        }

        async fn start(&self, _model: &str) -> Result<RuntimeEndpoint, HostError> {
            Ok(self.endpoint.clone())
        }

        async fn stop(&self, _model: &str) -> Result<(), HostError> {
            Ok(())
        }

        async fn status(&self) -> Result<HostStatus, HostError> {
            Ok(HostStatus {
                runtime: RuntimeStatus::Idle,
                endpoint: self.endpoint.clone(),
            })
        }
    }

    struct FakeCatalog;

    #[async_trait]
    impl Catalog for FakeCatalog {
        async fn models(&self) -> Result<Vec<CatalogModel>, ApplicationError> {
            Ok(vec![CatalogModel {
                id: "small".to_owned(),
                display_name: "Small".to_owned(),
                minimum_memory_bytes: 4,
                recommended_memory_bytes: 8,
            }])
        }
    }

    struct FakeRecommendation;

    impl RecommendationEngine for FakeRecommendation {
        fn recommend(
            &self,
            _hardware: &HardwareFacts,
            models: &[CatalogModel],
        ) -> Result<Recommendation, ApplicationError> {
            let model = models
                .first()
                .ok_or_else(|| ApplicationError::Recommendation("empty catalog".to_owned()))?;
            Ok(Recommendation {
                model_id: model.id.clone(),
                reason: "fits".to_owned(),
            })
        }
    }

    fn service(
        events: Arc<StdMutex<Vec<ProgressEvent>>>,
    ) -> Result<LumenService<FakeHost, FakeCatalog, FakeRecommendation>, Box<dyn std::error::Error>>
    {
        let base_url = Url::parse("http://127.0.0.1:11434")?;
        let sink = move |event| {
            if let Ok(mut values) = events.lock() {
                values.push(event);
            }
        };
        Ok(LumenService::new(
            Arc::new(FakeHost {
                endpoint: RuntimeEndpoint { base_url },
            }),
            Arc::new(FakeCatalog),
            Arc::new(FakeRecommendation),
            Arc::new(sink),
        ))
    }

    #[tokio::test]
    async fn recommendation_returns_to_idle() -> Result<(), Box<dyn std::error::Error>> {
        let events = Arc::new(StdMutex::new(Vec::new()));
        let service = service(Arc::clone(&events))?;
        let recommendation = service.recommend().await?;
        assert_eq!(recommendation.model_id, "small");
        assert_eq!(
            service.orchestration_state().await,
            OrchestrationState::Idle
        );
        Ok(())
    }

    #[tokio::test]
    async fn start_and_stop_track_state() -> Result<(), Box<dyn std::error::Error>> {
        let service = service(Arc::new(StdMutex::new(Vec::new())))?;
        service.start("small").await?;
        assert_eq!(
            service.orchestration_state().await,
            OrchestrationState::Running {
                model_id: "small".to_owned()
            }
        );
        service.stop().await?;
        assert_eq!(
            service.orchestration_state().await,
            OrchestrationState::Idle
        );
        Ok(())
    }

    #[test]
    fn hardware_error_is_preserved_by_host_error() {
        let error = HostError::from(ProbeError::InvalidData {
            interface: "test",
            detail: "failed".to_owned(),
        });
        assert!(matches!(error, HostError::Hardware(_)));
    }
}
