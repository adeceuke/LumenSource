//! Runtime abstraction, Ollama and dummy adapters, and verified binary installation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};

use async_trait::async_trait;
use futures_util::StreamExt;
pub use reqwest::Url;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};

/// Cooperative cancellation shared by the desktop command and long-running
/// runtime I/O. Cancellation is permanent and wakes an in-flight HTTP wait.
#[derive(Clone, Default)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
        self.state.notify.notify_waiters();
        self.state.notify.notify_one();
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), RuntimeError> {
        if self.is_cancelled() {
            Err(RuntimeError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.state.notify.notified().await;
    }
}

/// A progress notification emitted by a runtime operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeProgress {
    Downloading {
        downloaded: u64,
        total: Option<u64>,
    },
    Verifying,
    Installing,
    PullingModel {
        status: String,
        digest: Option<String>,
        completed: Option<u64>,
        total: Option<u64>,
    },
    Ready,
}

/// Receives progress without coupling runtime code to a UI framework.
pub trait ProgressReporter: Send + Sync {
    fn report(&self, progress: RuntimeProgress);
}

impl<F> ProgressReporter for F
where
    F: Fn(RuntimeProgress) + Send + Sync,
{
    fn report(&self, progress: RuntimeProgress) {
        self(progress);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatProgress {
    Content(String),
    Done,
}

pub trait ChatReporter: Send + Sync {
    fn report(&self, progress: ChatProgress);
}

impl<F> ChatReporter for F
where
    F: Fn(ChatProgress) + Send + Sync,
{
    fn report(&self, progress: ChatProgress) {
        self(progress);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEndpoint {
    pub base_url: Url,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeStatus {
    Unavailable,
    Idle,
    Running { models: Vec<String> },
}

/// A model present in a runtime's local model store, whether loaded or stopped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledModel {
    pub name: String,
    pub digest: Option<String>,
    pub size_bytes: Option<u64>,
}

/// Memory reserved for one model currently loaded by Ollama. These values
/// come from `/api/ps`; Ollama does not expose per-model processor utilization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelAllocation {
    pub name: String,
    pub total_memory_bytes: u64,
    pub vram_memory_bytes: u64,
    pub context_length: Option<u64>,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("invalid runtime URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("runtime returned HTTP {status}: {body}")]
    HttpStatus { status: StatusCode, body: String },
    #[error("runtime reported an error: {0}")]
    Remote(String),
    #[error("runtime response was invalid: {0}")]
    InvalidResponse(#[from] serde_json::Error),
    #[error("artifact checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("invalid artifact name `{0}`")]
    InvalidArtifactName(String),
    #[error("I/O operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP archive operation failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("runtime executable is already running")]
    AlreadyRunning,
    #[error("runtime executable is not running")]
    NotRunning,
    #[error("Ollama is not installed or is not available on PATH")]
    ExecutableNotFound,
    #[error("Ollama did not become healthy within the startup timeout")]
    StartupTimeout,
    #[error("Ollama is running outside Lumen Source and cannot be restarted by the app")]
    ExternallyManaged,
    #[error("installation cancelled")]
    Cancelled,
}

/// Operations required from an inference runtime.
#[async_trait]
pub trait Runtime: Send + Sync {
    async fn health(&self) -> Result<(), RuntimeError>;
    async fn installed_models(&self) -> Result<Vec<InstalledModel>, RuntimeError>;
    async fn pull_model(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
    ) -> Result<(), RuntimeError>;
    async fn delete_model(&self, model: &str) -> Result<(), RuntimeError>;
    async fn start(&self, model: &str) -> Result<(), RuntimeError>;
    async fn stop(&self, model: &str) -> Result<(), RuntimeError>;
    async fn status(&self) -> Result<RuntimeStatus, RuntimeError>;
    fn endpoint(&self) -> RuntimeEndpoint;
}

/// An in-memory runtime used to exercise the complete install/start/stop UI
/// without downloading a model or launching an inference server.
#[derive(Clone)]
pub struct DummyRuntime {
    endpoint: RuntimeEndpoint,
    state: Arc<Mutex<DummyState>>,
}

#[derive(Default)]
struct DummyState {
    installed: BTreeSet<String>,
    running: BTreeSet<String>,
}

impl DummyRuntime {
    pub fn new(base_url: &str) -> Result<Self, RuntimeError> {
        let mut url =
            Url::parse(base_url).map_err(|error| RuntimeError::InvalidUrl(error.to_string()))?;
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        Ok(Self {
            endpoint: RuntimeEndpoint { base_url: url },
            state: Arc::new(Mutex::new(DummyState::default())),
        })
    }
}

#[async_trait]
impl Runtime for DummyRuntime {
    async fn health(&self) -> Result<(), RuntimeError> {
        Ok(())
    }

    async fn installed_models(&self) -> Result<Vec<InstalledModel>, RuntimeError> {
        Ok(self
            .state
            .lock()
            .await
            .installed
            .iter()
            .map(|name| InstalledModel {
                name: name.clone(),
                digest: Some(format!("dummy:{name}")),
                size_bytes: Some(0),
            })
            .collect())
    }

    async fn pull_model(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
    ) -> Result<(), RuntimeError> {
        self.pull_model_cancellable(model, progress, &CancellationToken::new())
            .await
    }

    async fn delete_model(&self, model: &str) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().await;
        state.running.remove(model);
        state.installed.remove(model);
        Ok(())
    }

    async fn start(&self, model: &str) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().await;
        state.installed.insert(model.to_owned());
        state.running.insert(model.to_owned());
        Ok(())
    }

    async fn stop(&self, model: &str) -> Result<(), RuntimeError> {
        self.state.lock().await.running.remove(model);
        Ok(())
    }

    async fn status(&self) -> Result<RuntimeStatus, RuntimeError> {
        let models: Vec<String> = self.state.lock().await.running.iter().cloned().collect();
        if models.is_empty() {
            Ok(RuntimeStatus::Idle)
        } else {
            Ok(RuntimeStatus::Running { models })
        }
    }

    fn endpoint(&self) -> RuntimeEndpoint {
        self.endpoint.clone()
    }
}

impl DummyRuntime {
    pub async fn pull_model_cancellable(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
        cancellation: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        cancellation.check()?;
        progress.report(RuntimeProgress::PullingModel {
            status: "Simulating model installation…".to_owned(),
            digest: Some("dummy-model".to_owned()),
            completed: Some(1),
            total: Some(1),
        });
        cancellation.check()?;
        self.state.lock().await.installed.insert(model.to_owned());
        cancellation.check()?;
        progress.report(RuntimeProgress::Ready);
        Ok(())
    }
}

/// Ollama's fixed HTTP API adapter.
#[derive(Clone)]
pub struct OllamaRuntime {
    client: Client,
    endpoint: Arc<StdRwLock<RuntimeEndpoint>>,
    launched_process: Arc<Mutex<Option<Child>>>,
    executable: Arc<Mutex<PathBuf>>,
    server_environment: Arc<Mutex<BTreeMap<String, String>>>,
}

impl OllamaRuntime {
    pub fn new(base_url: &str) -> Result<Self, RuntimeError> {
        Self::new_with_executable(base_url, PathBuf::from("ollama"))
    }

    pub fn new_with_executable(base_url: &str, executable: PathBuf) -> Result<Self, RuntimeError> {
        Self::new_configured(base_url, executable, BTreeMap::new())
    }

    pub fn new_configured(
        base_url: &str,
        executable: PathBuf,
        server_environment: BTreeMap<String, String>,
    ) -> Result<Self, RuntimeError> {
        let mut url =
            Url::parse(base_url).map_err(|error| RuntimeError::InvalidUrl(error.to_string()))?;
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        Ok(Self {
            client: Client::new(),
            endpoint: Arc::new(StdRwLock::new(RuntimeEndpoint { base_url: url })),
            launched_process: Arc::new(Mutex::new(None)),
            executable: Arc::new(Mutex::new(executable)),
            server_environment: Arc::new(Mutex::new(server_environment)),
        })
    }

    pub fn with_client(base_url: Url, client: Client) -> Self {
        Self {
            client,
            endpoint: Arc::new(StdRwLock::new(RuntimeEndpoint { base_url })),
            launched_process: Arc::new(Mutex::new(None)),
            executable: Arc::new(Mutex::new(PathBuf::from("ollama"))),
            server_environment: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn api_url(&self, path: &str) -> Result<Url, RuntimeError> {
        self.endpoint
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .base_url
            .join(path)
            .map_err(|error| RuntimeError::InvalidUrl(error.to_string()))
    }

    async fn checked(response: reqwest::Response) -> Result<reqwest::Response, RuntimeError> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response
            .text()
            .await
            .unwrap_or_else(|error| error.to_string());
        Err(RuntimeError::HttpStatus { status, body })
    }

    /// Ensures the local Ollama server is reachable, starting the fixed
    /// `ollama serve` executable when necessary. No shell is involved.
    pub async fn ensure_running(&self) -> Result<(), RuntimeError> {
        self.ensure_running_cancellable(&CancellationToken::new())
            .await
    }

    pub async fn ensure_running_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        cancellation.check()?;
        let healthy = tokio::select! {
            _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
            result = self.health() => result.is_ok(),
        };
        if healthy {
            return Ok(());
        }

        cancellation.check()?;
        let mut process = self.launched_process.lock().await;
        if let Some(child) = process.as_mut() {
            if child.try_wait()?.is_some() {
                *process = None;
            }
        }
        if process.is_none() {
            let executable = self.executable.lock().await.clone();
            let environment = self.server_environment.lock().await.clone();
            let mut command = Command::new(executable);
            hide_console_window(&mut command);
            let child = command
                .arg("serve")
                .envs(environment)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        RuntimeError::ExecutableNotFound
                    } else {
                        RuntimeError::Io(error)
                    }
                })?;
            *process = Some(child);
        }
        drop(process);

        for _ in 0..50 {
            cancellation.check()?;
            let healthy = tokio::select! {
                _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
                result = self.health() => result.is_ok(),
            };
            if healthy {
                return Ok(());
            }
            tokio::select! {
                _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
                _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {},
            }
        }
        Err(RuntimeError::StartupTimeout)
    }

    pub async fn executable_available(&self) -> bool {
        let executable = self.executable.lock().await.clone();
        let mut command = Command::new(executable);
        hide_console_window(&mut command);
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok_and(|status| status.success())
    }

    pub async fn set_executable(&self, executable: PathBuf) {
        *self.executable.lock().await = executable;
    }

    pub async fn executable_path(&self) -> PathBuf {
        self.executable.lock().await.clone()
    }

    pub fn set_endpoint(&self, base_url: &str) -> Result<(), RuntimeError> {
        let mut url =
            Url::parse(base_url).map_err(|error| RuntimeError::InvalidUrl(error.to_string()))?;
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        *self
            .endpoint
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = RuntimeEndpoint { base_url: url };
        Ok(())
    }

    pub async fn set_server_environment(&self, environment: BTreeMap<String, String>) {
        *self.server_environment.lock().await = environment;
    }

    pub async fn managed_process_running(&self) -> bool {
        self.launched_process.lock().await.is_some()
    }

    /// Restarts only an Ollama process launched by this runtime adapter.
    /// A separately launched service is deliberately never terminated.
    pub async fn restart_managed_server(&self) -> Result<(), RuntimeError> {
        let had_managed_process = {
            let mut process = self.launched_process.lock().await;
            if process.is_some() {
                if let Some(child) = process.as_mut() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
                *process = None;
                true
            } else {
                false
            }
        };
        if !had_managed_process && self.health().await.is_ok() {
            return Err(RuntimeError::ExternallyManaged);
        }
        self.ensure_running().await
    }

    pub async fn version(&self) -> Result<String, RuntimeError> {
        let response = self.client.get(self.api_url("api/version")?).send().await?;
        let version: VersionResponse = Self::checked(response).await?.json().await?;
        Ok(version.version)
    }

    pub async fn pull_model_cancellable(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
        cancellation: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        cancellation.check()?;
        let request = self
            .client
            .post(self.api_url("api/pull")?)
            .json(&PullRequest {
                name: model,
                stream: true,
            })
            .send();
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
            response = request => response?,
        };
        let mut stream = Self::checked(response).await?.bytes_stream();
        let mut pending = Vec::new();
        loop {
            let chunk = tokio::select! {
                _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else {
                break;
            };
            pending.extend_from_slice(&chunk?);
            while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=index).collect();
                report_pull_line(&line, progress)?;
            }
        }
        cancellation.check()?;
        if !pending.is_empty() {
            report_pull_line(&pending, progress)?;
        }
        progress.report(RuntimeProgress::Ready);
        Ok(())
    }

    pub async fn chat_cancellable(
        &self,
        model: &str,
        messages: &[ChatMessage],
        reporter: &dyn ChatReporter,
        cancellation: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        cancellation.check()?;
        let request = self
            .client
            .post(self.api_url("api/chat")?)
            .json(&ChatRequest {
                model,
                messages,
                stream: true,
                keep_alive: -1,
            })
            .send();
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
            response = request => response?,
        };
        let mut stream = Self::checked(response).await?.bytes_stream();
        let mut pending = Vec::new();
        let mut done = false;
        loop {
            let chunk = tokio::select! {
                _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else {
                break;
            };
            pending.extend_from_slice(&chunk?);
            while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=index).collect();
                done |= report_chat_line(&line, reporter)?;
            }
        }
        cancellation.check()?;
        if !pending.is_empty() {
            done |= report_chat_line(&pending, reporter)?;
        }
        if !done {
            reporter.report(ChatProgress::Done);
        }
        Ok(())
    }

    /// Loads an embedding-only model and keeps it resident without routing it
    /// through Ollama's unsupported generation endpoint.
    pub async fn start_embedding(&self, model: &str) -> Result<(), RuntimeError> {
        self.set_embedding_keep_alive(model, -1).await
    }

    /// Unloads an embedding-only model through the same API that loaded it.
    pub async fn stop_embedding(&self, model: &str) -> Result<(), RuntimeError> {
        self.set_embedding_keep_alive(model, 0).await
    }

    async fn set_embedding_keep_alive(
        &self,
        model: &str,
        keep_alive: i64,
    ) -> Result<(), RuntimeError> {
        let response = self
            .client
            .post(self.api_url("api/embed")?)
            .json(&EmbedRequest {
                model,
                input: "LumenSource runtime check",
                keep_alive,
            })
            .send()
            .await?;
        Self::checked(response).await?;
        Ok(())
    }

    pub async fn model_allocation(
        &self,
        model: &str,
    ) -> Result<Option<ModelAllocation>, RuntimeError> {
        let response = self.client.get(self.api_url("api/ps")?).send().await?;
        let list: ProcessList = Self::checked(response).await?.json().await?;
        Ok(list
            .models
            .into_iter()
            .find(|loaded| same_model_reference(&loaded.name, model))
            .map(|loaded| ModelAllocation {
                name: loaded.name,
                total_memory_bytes: loaded.total_memory_bytes.unwrap_or_default(),
                vram_memory_bytes: loaded.vram_memory_bytes.unwrap_or_default(),
                context_length: loaded.context_length,
            }))
    }
}

#[derive(Serialize)]
struct PullRequest<'a> {
    name: &'a str,
    stream: bool,
}

#[derive(Serialize)]
struct DeleteRequest<'a> {
    model: &'a str,
}

#[derive(Deserialize)]
struct PullResponse {
    status: String,
    digest: Option<String>,
    completed: Option<u64>,
    total: Option<u64>,
    error: Option<String>,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    keep_alive: i64,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    message: Option<ChatResponseMessage>,
    #[serde(default)]
    done: bool,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    #[serde(default)]
    content: String,
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    keep_alive: i64,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
    keep_alive: i64,
}

#[derive(Deserialize)]
struct ProcessList {
    #[serde(default)]
    models: Vec<ProcessModel>,
}

#[derive(Deserialize)]
struct ProcessModel {
    name: String,
    #[serde(default, rename = "size")]
    total_memory_bytes: Option<u64>,
    #[serde(default, rename = "size_vram")]
    vram_memory_bytes: Option<u64>,
    #[serde(default)]
    context_length: Option<u64>,
}

#[derive(Deserialize)]
struct InstalledModelList {
    #[serde(default)]
    models: Vec<OllamaInstalledModel>,
}

#[derive(Deserialize)]
struct VersionResponse {
    version: String,
}

#[derive(Deserialize)]
struct OllamaInstalledModel {
    name: String,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default, rename = "size")]
    size_bytes: Option<u64>,
}

#[async_trait]
impl Runtime for OllamaRuntime {
    async fn health(&self) -> Result<(), RuntimeError> {
        let response = self.client.get(self.api_url("api/tags")?).send().await?;
        Self::checked(response).await?;
        Ok(())
    }

    async fn installed_models(&self) -> Result<Vec<InstalledModel>, RuntimeError> {
        let response = self.client.get(self.api_url("api/tags")?).send().await?;
        let list: InstalledModelList = Self::checked(response).await?.json().await?;
        Ok(list
            .models
            .into_iter()
            .map(|model| InstalledModel {
                name: model.name,
                digest: model.digest,
                size_bytes: model.size_bytes,
            })
            .collect())
    }

    async fn pull_model(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
    ) -> Result<(), RuntimeError> {
        self.pull_model_cancellable(model, progress, &CancellationToken::new())
            .await
    }

    async fn delete_model(&self, model: &str) -> Result<(), RuntimeError> {
        let response = self
            .client
            .delete(self.api_url("api/delete")?)
            .json(&DeleteRequest { model })
            .send()
            .await?;
        Self::checked(response).await?;
        Ok(())
    }

    async fn start(&self, model: &str) -> Result<(), RuntimeError> {
        let response = self
            .client
            .post(self.api_url("api/generate")?)
            .json(&GenerateRequest {
                model,
                prompt: "",
                stream: false,
                keep_alive: -1,
            })
            .send()
            .await?;
        Self::checked(response).await?;
        Ok(())
    }

    async fn stop(&self, model: &str) -> Result<(), RuntimeError> {
        let response = self
            .client
            .post(self.api_url("api/generate")?)
            .json(&GenerateRequest {
                model,
                prompt: "",
                stream: false,
                keep_alive: 0,
            })
            .send()
            .await?;
        Self::checked(response).await?;
        Ok(())
    }

    async fn status(&self) -> Result<RuntimeStatus, RuntimeError> {
        let response = match self.client.get(self.api_url("api/ps")?).send().await {
            Ok(response) => response,
            Err(error) if error.is_connect() => return Ok(RuntimeStatus::Unavailable),
            Err(error) => return Err(error.into()),
        };
        let response = Self::checked(response).await?;
        let list: ProcessList = response.json().await?;
        let models = list
            .models
            .into_iter()
            .map(|model| model.name)
            .collect::<Vec<_>>();
        if models.is_empty() {
            Ok(RuntimeStatus::Idle)
        } else {
            Ok(RuntimeStatus::Running { models })
        }
    }

    fn endpoint(&self) -> RuntimeEndpoint {
        self.endpoint
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

fn report_pull_line(line: &[u8], progress: &dyn ProgressReporter) -> Result<(), RuntimeError> {
    let line = line
        .strip_suffix(b"\n")
        .unwrap_or(line)
        .strip_suffix(b"\r")
        .unwrap_or(line);
    if line.is_empty() {
        return Ok(());
    }
    let update: PullResponse = serde_json::from_slice(line)?;
    if let Some(error) = update.error {
        return Err(RuntimeError::Remote(error));
    }
    progress.report(RuntimeProgress::PullingModel {
        status: update.status,
        digest: update.digest,
        completed: update.completed,
        total: update.total,
    });
    Ok(())
}

fn report_chat_line(line: &[u8], reporter: &dyn ChatReporter) -> Result<bool, RuntimeError> {
    let line = line
        .strip_suffix(b"\n")
        .unwrap_or(line)
        .strip_suffix(b"\r")
        .unwrap_or(line);
    if line.is_empty() {
        return Ok(false);
    }
    let update: ChatResponse = serde_json::from_slice(line)?;
    if let Some(error) = update.error {
        return Err(RuntimeError::Remote(error));
    }
    if let Some(message) = update.message {
        if !message.content.is_empty() {
            reporter.report(ChatProgress::Content(message.content));
        }
    }
    if update.done {
        reporter.report(ChatProgress::Done);
    }
    Ok(update.done)
}

fn same_model_reference(left: &str, right: &str) -> bool {
    normalize_model_reference(left) == normalize_model_reference(right)
}

fn normalize_model_reference(reference: &str) -> String {
    let reference = reference.trim();
    let last_slash = reference.rfind('/').map_or(0, |index| index + 1);
    if reference[last_slash..].contains(':') {
        reference.to_owned()
    } else {
        format!("{reference}:latest")
    }
}

#[derive(Clone, Debug)]
pub struct Artifact {
    pub url: Url,
    pub sha256: String,
    pub executable_name: String,
}

/// Downloads only the declared artifact, verifies it, then atomically installs it.
#[derive(Clone)]
pub struct ArtifactInstaller {
    client: Client,
}

impl Default for ArtifactInstaller {
    fn default() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

impl ArtifactInstaller {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn install(
        &self,
        artifact: &Artifact,
        install_dir: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<PathBuf, RuntimeError> {
        self.install_cancellable(artifact, install_dir, progress, &CancellationToken::new())
            .await
    }

    pub async fn install_cancellable(
        &self,
        artifact: &Artifact,
        install_dir: &Path,
        progress: &dyn ProgressReporter,
        cancellation: &CancellationToken,
    ) -> Result<PathBuf, RuntimeError> {
        validate_artifact_name(&artifact.executable_name)?;
        tokio::fs::create_dir_all(install_dir).await?;
        let destination = install_dir.join(&artifact.executable_name);
        let temporary = install_dir.join(format!("{}.download", artifact.executable_name));
        let result = async {
            let actual =
                Self::download_to_file(&self.client, artifact, &temporary, progress, cancellation)
                    .await?;
            cancellation.check()?;
            progress.report(RuntimeProgress::Verifying);
            if !actual.eq_ignore_ascii_case(&artifact.sha256) {
                return Err(RuntimeError::ChecksumMismatch {
                    expected: artifact.sha256.clone(),
                    actual,
                });
            }
            cancellation.check()?;
            progress.report(RuntimeProgress::Installing);
            set_executable(&temporary).await?;
            cancellation.check()?;
            tokio::fs::rename(&temporary, &destination).await?;
            progress.report(RuntimeProgress::Ready);
            Ok(destination)
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        result
    }

    /// Installs a verified `.tar.zst` runtime distribution without executing
    /// a downloaded installer script.
    pub async fn install_tar_zst(
        &self,
        artifact: &Artifact,
        install_dir: &Path,
        executable_relative_path: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<PathBuf, RuntimeError> {
        self.install_tar_zst_cancellable(
            artifact,
            install_dir,
            executable_relative_path,
            progress,
            &CancellationToken::new(),
        )
        .await
    }

    pub async fn install_tar_zst_cancellable(
        &self,
        artifact: &Artifact,
        install_dir: &Path,
        executable_relative_path: &Path,
        progress: &dyn ProgressReporter,
        cancellation: &CancellationToken,
    ) -> Result<PathBuf, RuntimeError> {
        validate_artifact_name(&artifact.executable_name)?;
        let parent = install_dir
            .parent()
            .ok_or_else(|| RuntimeError::InvalidArtifactName(install_dir.display().to_string()))?;
        tokio::fs::create_dir_all(parent).await?;
        let archive_path = parent.join(format!("{}.download", artifact.executable_name));
        let staging_dir = install_dir.with_extension("staging");
        if tokio::fs::try_exists(&staging_dir).await? {
            tokio::fs::remove_dir_all(&staging_dir).await?;
        }
        tokio::fs::create_dir_all(&staging_dir).await?;

        let result = async {
            let actual = Self::download_to_file(
                &self.client,
                artifact,
                &archive_path,
                progress,
                cancellation,
            )
            .await?;
            cancellation.check()?;
            progress.report(RuntimeProgress::Verifying);
            if !actual.eq_ignore_ascii_case(&artifact.sha256) {
                return Err(RuntimeError::ChecksumMismatch {
                    expected: artifact.sha256.clone(),
                    actual,
                });
            }

            cancellation.check()?;
            progress.report(RuntimeProgress::Installing);
            let archive = archive_path.clone();
            let destination = staging_dir.clone();
            let extraction_cancellation = cancellation.clone();
            tokio::task::spawn_blocking(move || -> Result<(), RuntimeError> {
                let file = std::fs::File::open(archive)?;
                let decoder = zstd::Decoder::new(file)?;
                let mut archive = tar::Archive::new(decoder);
                for entry in archive.entries()? {
                    extraction_cancellation.check()?;
                    let mut entry = entry?;
                    if !entry.unpack_in(&destination)? {
                        return Err(RuntimeError::InvalidArtifactName(
                            "archive entry escapes the installation directory".to_owned(),
                        ));
                    }
                }
                Ok(())
            })
            .await
            .map_err(|error| RuntimeError::Io(std::io::Error::other(error)))??;

            cancellation.check()?;
            let executable = staging_dir.join(executable_relative_path);
            if !tokio::fs::try_exists(&executable).await? {
                return Err(RuntimeError::InvalidArtifactName(format!(
                    "archive does not contain {}",
                    executable_relative_path.display()
                )));
            }
            set_executable(&executable).await?;
            cancellation.check()?;
            if tokio::fs::try_exists(install_dir).await? {
                tokio::fs::remove_dir_all(install_dir).await?;
            }
            tokio::fs::rename(&staging_dir, install_dir).await?;
            tokio::fs::remove_file(&archive_path).await?;
            progress.report(RuntimeProgress::Ready);
            Ok(install_dir.join(executable_relative_path))
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&archive_path).await;
            let _ = tokio::fs::remove_dir_all(&staging_dir).await;
        }
        result
    }

    /// Installs a verified ZIP runtime distribution without executing a
    /// downloaded installer.
    pub async fn install_zip(
        &self,
        artifact: &Artifact,
        install_dir: &Path,
        executable_relative_path: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<PathBuf, RuntimeError> {
        self.install_zip_cancellable(
            artifact,
            install_dir,
            executable_relative_path,
            progress,
            &CancellationToken::new(),
        )
        .await
    }

    pub async fn install_zip_cancellable(
        &self,
        artifact: &Artifact,
        install_dir: &Path,
        executable_relative_path: &Path,
        progress: &dyn ProgressReporter,
        cancellation: &CancellationToken,
    ) -> Result<PathBuf, RuntimeError> {
        validate_artifact_name(&artifact.executable_name)?;
        let parent = install_dir
            .parent()
            .ok_or_else(|| RuntimeError::InvalidArtifactName(install_dir.display().to_string()))?;
        tokio::fs::create_dir_all(parent).await?;
        let archive_path = parent.join(format!("{}.download", artifact.executable_name));
        let staging_dir = install_dir.with_extension("staging");
        if tokio::fs::try_exists(&staging_dir).await? {
            tokio::fs::remove_dir_all(&staging_dir).await?;
        }
        tokio::fs::create_dir_all(&staging_dir).await?;

        let result = async {
            let actual = Self::download_to_file(
                &self.client,
                artifact,
                &archive_path,
                progress,
                cancellation,
            )
            .await?;
            cancellation.check()?;
            progress.report(RuntimeProgress::Verifying);
            if !actual.eq_ignore_ascii_case(&artifact.sha256) {
                return Err(RuntimeError::ChecksumMismatch {
                    expected: artifact.sha256.clone(),
                    actual,
                });
            }

            cancellation.check()?;
            progress.report(RuntimeProgress::Installing);
            let archive = archive_path.clone();
            let destination = staging_dir.clone();
            let extraction_cancellation = cancellation.clone();
            tokio::task::spawn_blocking(move || -> Result<(), RuntimeError> {
                let file = std::fs::File::open(archive)?;
                let mut archive = zip::ZipArchive::new(file)?;
                for index in 0..archive.len() {
                    extraction_cancellation.check()?;
                    let mut entry = archive.by_index(index)?;
                    let relative_path = entry.enclosed_name().ok_or_else(|| {
                        RuntimeError::InvalidArtifactName(
                            "archive entry escapes the installation directory".to_owned(),
                        )
                    })?;
                    if entry
                        .unix_mode()
                        .is_some_and(|mode| mode & 0o170000 == 0o120000)
                    {
                        return Err(RuntimeError::InvalidArtifactName(
                            "ZIP archive contains a symbolic link".to_owned(),
                        ));
                    }
                    let output_path = destination.join(relative_path);
                    if entry.is_dir() {
                        std::fs::create_dir_all(&output_path)?;
                    } else {
                        if let Some(parent) = output_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut output = std::fs::File::create(output_path)?;
                        std::io::copy(&mut entry, &mut output)?;
                    }
                }
                Ok(())
            })
            .await
            .map_err(|error| RuntimeError::Io(std::io::Error::other(error)))??;

            cancellation.check()?;
            let executable = staging_dir.join(executable_relative_path);
            if !tokio::fs::try_exists(&executable).await? {
                return Err(RuntimeError::InvalidArtifactName(format!(
                    "archive does not contain {}",
                    executable_relative_path.display()
                )));
            }
            set_executable(&executable).await?;
            cancellation.check()?;
            if tokio::fs::try_exists(install_dir).await? {
                tokio::fs::remove_dir_all(install_dir).await?;
            }
            tokio::fs::rename(&staging_dir, install_dir).await?;
            tokio::fs::remove_file(&archive_path).await?;
            progress.report(RuntimeProgress::Ready);
            Ok(install_dir.join(executable_relative_path))
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&archive_path).await;
            let _ = tokio::fs::remove_dir_all(&staging_dir).await;
        }
        result
    }

    async fn download_to_file(
        client: &Client,
        artifact: &Artifact,
        destination: &Path,
        progress: &dyn ProgressReporter,
        cancellation: &CancellationToken,
    ) -> Result<String, RuntimeError> {
        cancellation.check()?;
        let request = client.get(artifact.url.clone()).send();
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
            response = request => response?,
        };
        let response = OllamaRuntime::checked(response).await?;
        let total = response.content_length();
        let mut file = tokio::fs::File::create(destination).await?;
        let mut stream = response.bytes_stream();
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        loop {
            let chunk = tokio::select! {
                _ = cancellation.cancelled() => return Err(RuntimeError::Cancelled),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            hasher.update(&chunk);
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            progress.report(RuntimeProgress::Downloading { downloaded, total });
        }
        cancellation.check()?;
        file.flush().await?;
        Ok(hex::encode(hasher.finalize()))
    }
}

fn validate_artifact_name(name: &str) -> Result<(), RuntimeError> {
    let path = Path::new(name);
    let mut components = path.components();
    let valid =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if valid {
        Ok(())
    } else {
        Err(RuntimeError::InvalidArtifactName(name.to_owned()))
    }
}

#[cfg(unix)]
async fn set_executable(path: &Path) -> Result<(), RuntimeError> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = tokio::fs::metadata(path).await?.permissions();
    permissions.set_mode(0o755);
    tokio::fs::set_permissions(path, permissions).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_executable(_path: &Path) -> Result<(), RuntimeError> {
    Ok(())
}

/// Launches one verified executable with predetermined arguments; no shell is used.
pub struct FixedExecutable {
    executable: PathBuf,
    arguments: Arc<[String]>,
    child: Option<Child>,
}

impl FixedExecutable {
    pub fn new(executable: PathBuf, arguments: Vec<String>) -> Self {
        Self {
            executable,
            arguments: arguments.into(),
            child: None,
        }
    }

    pub fn is_running(&mut self) -> Result<bool, RuntimeError> {
        match self.child.as_mut() {
            Some(child) => match child.try_wait()? {
                Some(_) => {
                    self.child = None;
                    Ok(false)
                }
                None => Ok(true),
            },
            None => Ok(false),
        }
    }

    pub fn launch(&mut self) -> Result<(), RuntimeError> {
        if self.is_running()? {
            return Err(RuntimeError::AlreadyRunning);
        }
        let mut command = Command::new(&self.executable);
        hide_console_window(&mut command);
        let child = command
            .args(self.arguments.iter())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        self.child = Some(child);
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<(), RuntimeError> {
        let mut child = self.child.take().ok_or(RuntimeError::NotRunning)?;
        child.kill().await?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn hide_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_name_must_be_a_single_component() {
        assert!(validate_artifact_name("ollama").is_ok());
        assert!(validate_artifact_name("../ollama").is_err());
        assert!(validate_artifact_name("bin/ollama").is_err());
        assert!(validate_artifact_name("").is_err());
    }

    #[test]
    fn pull_progress_parses_ndjson_line() {
        let updates = std::sync::Mutex::new(Vec::new());
        let reporter = |progress| {
            if let Ok(mut values) = updates.lock() {
                values.push(progress);
            }
        };
        let result = report_pull_line(
            br#"{"status":"pulling","digest":"sha256:abc","completed":2,"total":4}"#,
            &reporter,
        );
        assert!(result.is_ok());
        let Ok(values) = updates.lock() else {
            panic!("progress values should remain available");
        };
        assert_eq!(
            values.as_slice(),
            &[RuntimeProgress::PullingModel {
                status: "pulling".to_owned(),
                digest: Some("sha256:abc".to_owned()),
                completed: Some(2),
                total: Some(4),
            }]
        );
    }

    #[test]
    fn chat_progress_parses_content_and_completion_lines() {
        let updates = std::sync::Mutex::new(Vec::new());
        let reporter = |progress| {
            if let Ok(mut values) = updates.lock() {
                values.push(progress);
            }
        };

        let content = report_chat_line(
            br#"{"message":{"role":"assistant","content":"Hello"},"done":false}"#,
            &reporter,
        );
        let done = report_chat_line(
            br#"{"message":{"role":"assistant","content":""},"done":true}"#,
            &reporter,
        );

        assert!(matches!(content, Ok(false)));
        assert!(matches!(done, Ok(true)));
        let Ok(values) = updates.lock() else {
            panic!("chat progress values should remain available");
        };
        assert_eq!(
            values.as_slice(),
            &[
                ChatProgress::Content("Hello".to_owned()),
                ChatProgress::Done
            ]
        );
    }

    #[test]
    fn ollama_delete_request_uses_the_runtime_model_identifier() {
        let Ok(serialized) = serde_json::to_value(DeleteRequest {
            model: "qwen2.5:latest",
        }) else {
            panic!("delete request should serialize");
        };

        assert_eq!(serialized, serde_json::json!({ "model": "qwen2.5:latest" }));
    }

    #[test]
    fn ollama_embedding_lifecycle_request_uses_embed_fields() {
        let Ok(serialized) = serde_json::to_value(EmbedRequest {
            model: "bge-m3:567m",
            input: "LumenSource runtime check",
            keep_alive: -1,
        }) else {
            panic!("embedding lifecycle request should serialize");
        };

        assert_eq!(
            serialized,
            serde_json::json!({
                "model": "bge-m3:567m",
                "input": "LumenSource runtime check",
                "keep_alive": -1
            })
        );
    }

    #[test]
    fn parses_installed_models_from_ollama_tags_shape() {
        let parsed = serde_json::from_slice::<InstalledModelList>(
            br#"{
            "models": [
                {
                    "name": "qwen2.5-coder:14b",
                    "model": "qwen2.5-coder:14b",
                    "modified_at": "2026-07-21T10:00:00Z",
                    "size": 9876543210,
                    "digest": "sha256:abc"
                },
                { "name": "small:latest" }
            ]
        }"#,
        );
        let Ok(list) = parsed else {
            panic!("valid Ollama tags response should parse");
        };

        assert_eq!(list.models.len(), 2);
        assert_eq!(list.models[0].name, "qwen2.5-coder:14b");
        assert_eq!(list.models[0].size_bytes, Some(9_876_543_210));
        assert_eq!(list.models[0].digest.as_deref(), Some("sha256:abc"));
        assert_eq!(list.models[1].size_bytes, None);
    }

    #[test]
    fn parses_per_model_memory_from_ollama_process_shape() {
        let parsed = serde_json::from_slice::<ProcessList>(
            br#"{
                "models": [{
                    "name": "qwen2.5-coder:14b",
                    "size": 10000,
                    "size_vram": 7500,
                    "context_length": 32768
                }]
            }"#,
        );
        let Ok(list) = parsed else {
            panic!("valid Ollama process response should parse");
        };

        assert_eq!(list.models.len(), 1);
        assert_eq!(list.models[0].total_memory_bytes, Some(10_000));
        assert_eq!(list.models[0].vram_memory_bytes, Some(7_500));
        assert_eq!(list.models[0].context_length, Some(32_768));
        assert!(same_model_reference(
            "qwen2.5-coder",
            "qwen2.5-coder:latest"
        ));
    }

    #[tokio::test]
    async fn dummy_runtime_has_a_complete_in_memory_lifecycle() {
        let Ok(runtime) = DummyRuntime::new("http://127.0.0.1:9999") else {
            panic!("fixed dummy URL should be valid");
        };
        let events = std::sync::Mutex::new(Vec::new());
        let reporter = |event| {
            if let Ok(mut events) = events.lock() {
                events.push(event);
            }
        };

        assert!(runtime.pull_model("dummy:latest", &reporter).await.is_ok());
        assert!(runtime.start("dummy:latest").await.is_ok());
        assert!(matches!(
            runtime.status().await,
            Ok(RuntimeStatus::Running { models }) if models == ["dummy:latest"]
        ));
        assert!(runtime.stop("dummy:latest").await.is_ok());
        assert!(matches!(runtime.status().await, Ok(RuntimeStatus::Idle)));
        assert!(matches!(
            runtime.installed_models().await,
            Ok(models) if models.len() == 1 && models[0].name == "dummy:latest"
        ));
        assert!(runtime.delete_model("dummy:latest").await.is_ok());
        assert!(matches!(
            runtime.installed_models().await,
            Ok(models) if models.is_empty()
        ));
    }

    #[tokio::test]
    async fn cancellation_wakes_waiters_and_prevents_dummy_installation() {
        let cancellation = CancellationToken::new();
        let waiter = cancellation.clone();
        let waiting = tokio::spawn(async move {
            waiter.cancelled().await;
        });
        cancellation.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
                .await
                .is_ok()
        );

        let Ok(runtime) = DummyRuntime::new("http://127.0.0.1:9999") else {
            panic!("fixed dummy URL should be valid");
        };
        let reporter = |_: RuntimeProgress| {};
        assert!(matches!(
            runtime
                .pull_model_cancellable("cancelled:latest", &reporter, &cancellation)
                .await,
            Err(RuntimeError::Cancelled)
        ));
        assert!(matches!(
            runtime.installed_models().await,
            Ok(models) if models.is_empty()
        ));
    }
}
