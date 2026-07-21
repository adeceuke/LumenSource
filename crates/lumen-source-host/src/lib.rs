//! Host abstraction and local-machine composition.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
pub use lumen_source_hardware::{HardwareFacts, UsageSnapshot};
use lumen_source_hardware::{HardwareProbe, ProbeError};
use lumen_source_runtime::{
    Artifact, ArtifactInstaller, ProgressReporter, Runtime, RuntimeEndpoint, RuntimeError,
    RuntimeStatus,
};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct RuntimeInstall {
    pub artifact: Artifact,
    pub install_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostStatus {
    pub runtime: RuntimeStatus,
    pub endpoint: RuntimeEndpoint,
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error(transparent)]
    Hardware(#[from] ProbeError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// Machine operations needed by application orchestration.
#[async_trait]
pub trait Host: Send + Sync {
    async fn hardware_facts(&self) -> Result<HardwareFacts, HostError>;
    async fn hardware_usage(&self) -> Result<UsageSnapshot, HostError>;
    async fn install_runtime(
        &self,
        install: &RuntimeInstall,
        progress: &dyn ProgressReporter,
    ) -> Result<PathBuf, HostError>;
    async fn install_model(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
    ) -> Result<(), HostError>;
    async fn start(&self, model: &str) -> Result<RuntimeEndpoint, HostError>;
    async fn stop(&self, model: &str) -> Result<(), HostError>;
    async fn status(&self) -> Result<HostStatus, HostError>;
}

/// A local host delegates hardware and runtime concerns to their adapters.
pub struct LocalHost<P, R> {
    probe: Arc<P>,
    runtime: Arc<R>,
    installer: ArtifactInstaller,
}

impl<P, R> LocalHost<P, R>
where
    P: HardwareProbe,
    R: Runtime,
{
    pub fn new(probe: Arc<P>, runtime: Arc<R>) -> Self {
        Self {
            probe,
            runtime,
            installer: ArtifactInstaller::default(),
        }
    }

    pub fn with_installer(probe: Arc<P>, runtime: Arc<R>, installer: ArtifactInstaller) -> Self {
        Self {
            probe,
            runtime,
            installer,
        }
    }
}

#[async_trait]
impl<P, R> Host for LocalHost<P, R>
where
    P: HardwareProbe + 'static,
    R: Runtime + 'static,
{
    async fn hardware_facts(&self) -> Result<HardwareFacts, HostError> {
        Ok(self.probe.hardware_facts().await?)
    }

    async fn hardware_usage(&self) -> Result<UsageSnapshot, HostError> {
        Ok(self.probe.usage_snapshot().await?)
    }

    async fn install_runtime(
        &self,
        install: &RuntimeInstall,
        progress: &dyn ProgressReporter,
    ) -> Result<PathBuf, HostError> {
        Ok(self
            .installer
            .install(&install.artifact, &install.install_dir, progress)
            .await?)
    }

    async fn install_model(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
    ) -> Result<(), HostError> {
        Ok(self.runtime.pull_model(model, progress).await?)
    }

    async fn start(&self, model: &str) -> Result<RuntimeEndpoint, HostError> {
        self.runtime.start(model).await?;
        Ok(self.runtime.endpoint())
    }

    async fn stop(&self, model: &str) -> Result<(), HostError> {
        Ok(self.runtime.stop(model).await?)
    }

    async fn status(&self) -> Result<HostStatus, HostError> {
        Ok(HostStatus {
            runtime: self.runtime.status().await?,
            endpoint: self.runtime.endpoint(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_source_hardware::{CpuFacts, OsFacts, StorageFacts};
    use lumen_source_runtime::{RuntimeProgress, Url};
    use std::sync::Mutex;

    struct FakeProbe;

    #[async_trait]
    impl HardwareProbe for FakeProbe {
        async fn hardware_facts(&self) -> Result<HardwareFacts, ProbeError> {
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
                    logical_cores: 8,
                    physical_cores: Some(4),
                },
                total_ram_bytes: 16,
                available_ram_bytes: 12,
                storage: StorageFacts {
                    mount_point: PathBuf::from("/"),
                    total_bytes: 100,
                    available_bytes: 80,
                },
                accelerators: Vec::new(),
            })
        }

        async fn usage_snapshot(&self) -> Result<UsageSnapshot, ProbeError> {
            Ok(UsageSnapshot {
                sampled_at_unix_ms: 0,
                cpu_utilization_percent: 25.0,
                used_ram_bytes: 4,
                available_ram_bytes: 12,
                accelerators: Vec::new(),
            })
        }
    }

    struct FakeRuntime {
        calls: Mutex<Vec<String>>,
        endpoint: RuntimeEndpoint,
    }

    impl FakeRuntime {
        fn create() -> Result<Self, RuntimeError> {
            let base_url = Url::parse("http://127.0.0.1:11434")
                .map_err(|error| RuntimeError::InvalidUrl(error.to_string()))?;
            Ok(Self {
                calls: Mutex::new(Vec::new()),
                endpoint: RuntimeEndpoint { base_url },
            })
        }

        fn record(&self, call: &str) {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(call.to_owned());
            }
        }
    }

    #[async_trait]
    impl Runtime for FakeRuntime {
        async fn health(&self) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn pull_model(
            &self,
            model: &str,
            progress: &dyn ProgressReporter,
        ) -> Result<(), RuntimeError> {
            self.record(&format!("pull:{model}"));
            progress.report(RuntimeProgress::Ready);
            Ok(())
        }

        async fn start(&self, model: &str) -> Result<(), RuntimeError> {
            self.record(&format!("start:{model}"));
            Ok(())
        }

        async fn stop(&self, model: &str) -> Result<(), RuntimeError> {
            self.record(&format!("stop:{model}"));
            Ok(())
        }

        async fn status(&self) -> Result<RuntimeStatus, RuntimeError> {
            Ok(RuntimeStatus::Idle)
        }

        fn endpoint(&self) -> RuntimeEndpoint {
            self.endpoint.clone()
        }
    }

    #[tokio::test]
    async fn local_host_composes_probe_and_runtime() -> Result<(), Box<dyn std::error::Error>> {
        let runtime = Arc::new(FakeRuntime::create()?);
        let host = LocalHost::new(Arc::new(FakeProbe), Arc::clone(&runtime));
        let facts = host.hardware_facts().await?;
        let endpoint = host.start("model").await?;
        assert_eq!(facts.cpu.logical_cores, 8);
        assert_eq!(endpoint, runtime.endpoint);
        let calls = runtime.calls.lock().map_err(|error| error.to_string())?;
        assert_eq!(calls.as_slice(), ["start:model"]);
        Ok(())
    }
}
