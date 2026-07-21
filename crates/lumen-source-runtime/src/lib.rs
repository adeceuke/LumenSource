//! Runtime abstraction, the Ollama adapter, and verified binary installation.

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
pub use reqwest::Url;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

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
    #[error("runtime executable is already running")]
    AlreadyRunning,
    #[error("runtime executable is not running")]
    NotRunning,
    #[error("Ollama is not installed or is not available on PATH")]
    ExecutableNotFound,
    #[error("Ollama did not become healthy within the startup timeout")]
    StartupTimeout,
}

/// Operations required from an inference runtime.
#[async_trait]
pub trait Runtime: Send + Sync {
    async fn health(&self) -> Result<(), RuntimeError>;
    async fn pull_model(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
    ) -> Result<(), RuntimeError>;
    async fn start(&self, model: &str) -> Result<(), RuntimeError>;
    async fn stop(&self, model: &str) -> Result<(), RuntimeError>;
    async fn status(&self) -> Result<RuntimeStatus, RuntimeError>;
    fn endpoint(&self) -> RuntimeEndpoint;
}

/// Ollama's fixed HTTP API adapter.
#[derive(Clone)]
pub struct OllamaRuntime {
    client: Client,
    endpoint: RuntimeEndpoint,
    owned_process: Arc<Mutex<Option<Child>>>,
    executable: Arc<Mutex<PathBuf>>,
}

impl OllamaRuntime {
    pub fn new(base_url: &str) -> Result<Self, RuntimeError> {
        Self::new_with_executable(base_url, PathBuf::from("ollama"))
    }

    pub fn new_with_executable(base_url: &str, executable: PathBuf) -> Result<Self, RuntimeError> {
        let mut url =
            Url::parse(base_url).map_err(|error| RuntimeError::InvalidUrl(error.to_string()))?;
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        Ok(Self {
            client: Client::new(),
            endpoint: RuntimeEndpoint { base_url: url },
            owned_process: Arc::new(Mutex::new(None)),
            executable: Arc::new(Mutex::new(executable)),
        })
    }

    pub fn with_client(base_url: Url, client: Client) -> Self {
        Self {
            client,
            endpoint: RuntimeEndpoint { base_url },
            owned_process: Arc::new(Mutex::new(None)),
            executable: Arc::new(Mutex::new(PathBuf::from("ollama"))),
        }
    }

    fn api_url(&self, path: &str) -> Result<Url, RuntimeError> {
        self.endpoint
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
        if self.health().await.is_ok() {
            return Ok(());
        }

        let mut process = self.owned_process.lock().await;
        if process.is_none() {
            let executable = self.executable.lock().await.clone();
            let child = Command::new(executable)
                .arg("serve")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
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
            if self.health().await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        Err(RuntimeError::StartupTimeout)
    }

    pub async fn executable_available(&self) -> bool {
        let executable = self.executable.lock().await.clone();
        Command::new(executable)
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
}

#[derive(Serialize)]
struct PullRequest<'a> {
    name: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct PullResponse {
    status: String,
    completed: Option<u64>,
    total: Option<u64>,
    error: Option<String>,
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
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
}

#[async_trait]
impl Runtime for OllamaRuntime {
    async fn health(&self) -> Result<(), RuntimeError> {
        let response = self.client.get(self.api_url("api/tags")?).send().await?;
        Self::checked(response).await?;
        Ok(())
    }

    async fn pull_model(
        &self,
        model: &str,
        progress: &dyn ProgressReporter,
    ) -> Result<(), RuntimeError> {
        let response = self
            .client
            .post(self.api_url("api/pull")?)
            .json(&PullRequest {
                name: model,
                stream: true,
            })
            .send()
            .await?;
        let mut stream = Self::checked(response).await?.bytes_stream();
        let mut pending = Vec::new();
        while let Some(chunk) = stream.next().await {
            pending.extend_from_slice(&chunk?);
            while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=index).collect();
                report_pull_line(&line, progress)?;
            }
        }
        if !pending.is_empty() {
            report_pull_line(&pending, progress)?;
        }
        progress.report(RuntimeProgress::Ready);
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
        self.endpoint.clone()
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
        completed: update.completed,
        total: update.total,
    });
    Ok(())
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
        validate_artifact_name(&artifact.executable_name)?;
        tokio::fs::create_dir_all(install_dir).await?;
        let destination = install_dir.join(&artifact.executable_name);
        let temporary = install_dir.join(format!("{}.download", artifact.executable_name));
        let response = Self::download_response(&self.client, artifact).await?;
        let total = response.content_length();
        let mut file = tokio::fs::File::create(&temporary).await?;
        let mut stream = response.bytes_stream();
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            hasher.update(&chunk);
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            progress.report(RuntimeProgress::Downloading { downloaded, total });
        }
        file.flush().await?;
        drop(file);
        progress.report(RuntimeProgress::Verifying);
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(&artifact.sha256) {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(RuntimeError::ChecksumMismatch {
                expected: artifact.sha256.clone(),
                actual,
            });
        }
        progress.report(RuntimeProgress::Installing);
        set_executable(&temporary).await?;
        tokio::fs::rename(&temporary, &destination).await?;
        progress.report(RuntimeProgress::Ready);
        Ok(destination)
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

        let response = Self::download_response(&self.client, artifact).await?;
        let total = response.content_length();
        let mut file = tokio::fs::File::create(&archive_path).await?;
        let mut stream = response.bytes_stream();
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            hasher.update(&chunk);
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            progress.report(RuntimeProgress::Downloading { downloaded, total });
        }
        file.flush().await?;
        drop(file);

        progress.report(RuntimeProgress::Verifying);
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(&artifact.sha256) {
            let _ = tokio::fs::remove_file(&archive_path).await;
            let _ = tokio::fs::remove_dir_all(&staging_dir).await;
            return Err(RuntimeError::ChecksumMismatch {
                expected: artifact.sha256.clone(),
                actual,
            });
        }

        progress.report(RuntimeProgress::Installing);
        let archive = archive_path.clone();
        let destination = staging_dir.clone();
        tokio::task::spawn_blocking(move || -> Result<(), RuntimeError> {
            let file = std::fs::File::open(archive)?;
            let decoder = zstd::Decoder::new(file)?;
            let mut archive = tar::Archive::new(decoder);
            for entry in archive.entries()? {
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

        let executable = staging_dir.join(executable_relative_path);
        if !tokio::fs::try_exists(&executable).await? {
            return Err(RuntimeError::InvalidArtifactName(format!(
                "archive does not contain {}",
                executable_relative_path.display()
            )));
        }
        set_executable(&executable).await?;
        if tokio::fs::try_exists(install_dir).await? {
            tokio::fs::remove_dir_all(install_dir).await?;
        }
        tokio::fs::rename(&staging_dir, install_dir).await?;
        tokio::fs::remove_file(&archive_path).await?;
        progress.report(RuntimeProgress::Ready);
        Ok(install_dir.join(executable_relative_path))
    }

    async fn download_response(
        client: &Client,
        artifact: &Artifact,
    ) -> Result<reqwest::Response, RuntimeError> {
        let response = client.get(artifact.url.clone()).send().await?;
        OllamaRuntime::checked(response).await
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
        let child = Command::new(&self.executable)
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
            br#"{"status":"pulling","completed":2,"total":4}"#,
            &reporter,
        );
        assert!(result.is_ok());
        let count = updates
            .lock()
            .map(|values| values.len())
            .unwrap_or_default();
        assert_eq!(count, 1);
    }
}
