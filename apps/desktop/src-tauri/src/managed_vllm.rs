use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use lumen_source_runtime::VllmRuntime;
use serde::Serialize;
use tokio::process::Command;
use zeroize::Zeroizing;

use crate::settings::{ModelInferenceTask, ModelSettings, VllmSettings};

pub const MANAGED_VLLM_IMAGE: &str = "vllm/vllm-openai:v0.23.0";
const HUGGING_FACE_VOLUME: &str = "lumensource-huggingface";
const VLLM_CACHE_VOLUME: &str = "lumensource-vllm-cache";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedVllmSupport {
    pub supported: bool,
    pub platform: String,
    pub container_engine: Option<String>,
    pub nvidia_available: bool,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedContainerInventory {
    pub name: String,
    pub entry_id: String,
    pub model_id: String,
    pub served_model_name: String,
    pub port: Option<u16>,
    pub running: bool,
}

#[derive(Clone, Debug)]
pub struct ManagedVllmSpec {
    pub entry_id: String,
    pub model_id: String,
    pub served_model_name: String,
    pub port: u16,
    pub settings: ModelSettings,
    pub defaults: VllmSettings,
}

impl ManagedVllmSpec {
    pub fn container_name(&self) -> String {
        let safe = self
            .entry_id
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .take(24)
            .collect::<String>()
            .to_ascii_lowercase();
        format!("lumensource-vllm-{safe}")
    }

    fn runtime_arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "--model".to_owned(),
            self.model_id.clone(),
            "--served-model-name".to_owned(),
            self.served_model_name.clone(),
            "--host".to_owned(),
            "0.0.0.0".to_owned(),
            "--port".to_owned(),
            "8000".to_owned(),
            "--disable-fastapi-docs".to_owned(),
            "--gpu-memory-utilization".to_owned(),
            self.settings
                .vllm_gpu_memory_utilization
                .unwrap_or(self.defaults.gpu_memory_utilization)
                .to_string(),
            "--max-num-seqs".to_owned(),
            self.settings
                .vllm_max_concurrent_sequences
                .unwrap_or(self.defaults.max_concurrent_sequences)
                .to_string(),
            "--dtype".to_owned(),
            self.settings
                .vllm_weight_dtype
                .clone()
                .unwrap_or_else(|| self.defaults.weight_dtype.clone()),
            "--kv-cache-dtype".to_owned(),
            self.settings
                .vllm_kv_cache_dtype
                .clone()
                .unwrap_or_else(|| self.defaults.kv_cache_dtype.clone()),
            "--tensor-parallel-size".to_owned(),
            self.settings
                .vllm_tensor_parallel_size
                .unwrap_or(self.defaults.tensor_parallel_size)
                .to_string(),
            "--pipeline-parallel-size".to_owned(),
            self.settings
                .vllm_pipeline_parallel_size
                .unwrap_or(self.defaults.pipeline_parallel_size)
                .to_string(),
        ];
        if let Some(revision) = self.settings.vllm_model_revision.as_deref() {
            arguments.extend(["--revision".to_owned(), revision.to_owned()]);
        }
        if let Some(revision) = self.settings.vllm_tokenizer_revision.as_deref() {
            arguments.extend(["--tokenizer-revision".to_owned(), revision.to_owned()]);
        }
        if let Some(runner) = self
            .settings
            .vllm_runner
            .as_deref()
            .or(self.settings.vllm_task.as_deref())
        {
            arguments.extend(["--runner".to_owned(), runner.to_owned()]);
        }
        if let Some(quantization) = self
            .settings
            .vllm_quantization
            .as_deref()
            .or(self.defaults.quantization.as_deref())
        {
            arguments.extend(["--quantization".to_owned(), quantization.to_owned()]);
        }
        if let Some(context) = self
            .settings
            .context_length
            .or(self.defaults.max_context_length)
        {
            arguments.extend(["--max-model-len".to_owned(), context.to_string()]);
        }
        let prefix_caching = self
            .settings
            .vllm_prefix_caching
            .unwrap_or(self.defaults.prefix_caching);
        arguments.push(if prefix_caching {
            "--enable-prefix-caching".to_owned()
        } else {
            "--no-enable-prefix-caching".to_owned()
        });
        let offload = self
            .settings
            .vllm_cpu_offload_gib
            .unwrap_or(self.defaults.cpu_offload_gib);
        if offload > 0.0 {
            arguments.extend(["--cpu-offload-gb".to_owned(), offload.to_string()]);
        }
        arguments
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContainerEngine {
    Docker,
    Podman,
}

impl ContainerEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Podman => "podman",
        }
    }

    fn gpu_arguments(self) -> &'static [&'static str] {
        match self {
            Self::Docker => &["--gpus", "all"],
            Self::Podman => &["--device", "nvidia.com/gpu=all"],
        }
    }
}

pub async fn detect_support() -> ManagedVllmSupport {
    #[cfg(not(target_os = "linux"))]
    {
        ManagedVllmSupport {
            supported: false,
            platform: std::env::consts::OS.to_owned(),
            container_engine: None,
            nvidia_available: false,
            message: "Managed vLLM requires a Linux host with an NVIDIA GPU. Connect an external vLLM service from Windows or macOS.".to_owned(),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let nvidia_available = command_succeeds("nvidia-smi", &["-L"]).await;
        let engine =
            if command_succeeds("docker", &["version", "--format", "{{.Server.Version}}"]).await {
                Some(ContainerEngine::Docker)
            } else if command_succeeds("podman", &["version", "--format", "{{.Version}}"]).await {
                Some(ContainerEngine::Podman)
            } else {
                None
            };
        let gpu_runtime = match engine {
            Some(ContainerEngine::Docker) => {
                command_output("docker", &["info", "--format", "{{json .Runtimes}}"])
                    .await
                    .is_ok_and(|output| output.to_ascii_lowercase().contains("nvidia"))
            }
            Some(ContainerEngine::Podman) => command_succeeds("nvidia-ctk", &["cdi", "list"]).await,
            None => false,
        };
        let supported = nvidia_available && engine.is_some() && gpu_runtime;
        ManagedVllmSupport {
            supported,
            platform: "linux".to_owned(),
            container_engine: engine.map(|engine| engine.as_str().to_owned()),
            nvidia_available,
            message: if supported {
                "Linux, NVIDIA, and the GPU container runtime are ready.".to_owned()
            } else {
                "Install and configure Docker or Podman plus NVIDIA container support outside Lumen Source, then retry. Lumen Source never installs drivers or container tooling.".to_owned()
            },
        }
    }
}

pub async fn launch(
    engine: ContainerEngine,
    spec: &ManagedVllmSpec,
    hugging_face_token: Option<&Zeroizing<String>>,
) -> Result<(), String> {
    ensure_linux()?;
    remove_container(engine, &spec.container_name()).await?;
    let mut command = Command::new(engine.as_str());
    command
        .arg("run")
        .arg("--detach")
        .arg("--name")
        .arg(spec.container_name())
        .arg("--label")
        .arg("dev.lumensource.managed=vllm")
        .arg("--label")
        .arg(format!("dev.lumensource.entry-id={}", spec.entry_id))
        .arg("--label")
        .arg(format!("dev.lumensource.model-id={}", spec.model_id))
        .arg("--label")
        .arg(format!(
            "dev.lumensource.served-model={}",
            spec.served_model_name
        ))
        .arg("--label")
        .arg(format!("dev.lumensource.port={}", spec.port))
        .arg("--restart")
        .arg("unless-stopped");
    command.args(engine.gpu_arguments());
    command
        .arg("--ipc")
        .arg("host")
        .arg("--publish")
        .arg(format!("127.0.0.1:{}:8000", spec.port))
        .arg("--volume")
        .arg(format!("{HUGGING_FACE_VOLUME}:/root/.cache/huggingface"))
        .arg("--volume")
        .arg(format!("{VLLM_CACHE_VOLUME}:/root/.cache/vllm"));
    if let Some(token) = hugging_face_token {
        command
            .arg("--env")
            .arg("HF_TOKEN")
            .env("HF_TOKEN", token.as_str());
    }
    command
        .arg(MANAGED_VLLM_IMAGE)
        .args(spec.runtime_arguments());
    let output = command
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("Could not launch {engine:?}: {error}"))?;
    if !output.status.success() {
        return Err(sanitized_command_error(
            "launch the vLLM container",
            &output,
        ));
    }
    if let Err(error) = wait_until_healthy(spec.port, &spec.served_model_name, 180).await {
        let recent_logs = logs(engine, &spec.container_name(), 30)
            .await
            .unwrap_or_default();
        return Err(format!(
            "{error} {}",
            recent_logs.last().cloned().unwrap_or_else(|| {
                "Inspect the container logs for the model-loading error.".to_owned()
            })
        ));
    }
    Ok(())
}

pub async fn wait_until_healthy(
    port: u16,
    served_model_name: &str,
    attempts: usize,
) -> Result<(), String> {
    let runtime = VllmRuntime::new(
        &format!("http://127.0.0.1:{port}"),
        true,
        Duration::from_secs(2),
        Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    for _ in 0..attempts {
        if runtime
            .models(None)
            .await
            .is_ok_and(|models| models.iter().any(|model| model == served_model_name))
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("The managed vLLM server did not become healthy before the timeout.".to_owned())
}

pub async fn start(engine: ContainerEngine, name: &str) -> Result<(), String> {
    run_checked(engine, &["start", name], "start the managed vLLM container").await
}

pub async fn stop(engine: ContainerEngine, name: &str) -> Result<(), String> {
    run_checked(
        engine,
        &["stop", "--time", "20", name],
        "stop the managed vLLM container",
    )
    .await
}

pub async fn remove_container(engine: ContainerEngine, name: &str) -> Result<(), String> {
    let output = Command::new(engine.as_str())
        .args(["rm", "--force", name])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("Could not inspect the managed vLLM container: {error}"))?;
    if output.status.success()
        || String::from_utf8_lossy(&output.stderr)
            .to_ascii_lowercase()
            .contains("no such")
    {
        Ok(())
    } else {
        Err(sanitized_command_error(
            "remove the managed vLLM container",
            &output,
        ))
    }
}

pub async fn is_running(engine: ContainerEngine, name: &str) -> Result<bool, String> {
    let output = command_output(
        engine.as_str(),
        &["inspect", "--format", "{{.State.Running}}", name],
    )
    .await?;
    Ok(output.trim() == "true")
}

pub async fn logs(
    engine: ContainerEngine,
    name: &str,
    limit: usize,
) -> Result<Vec<String>, String> {
    let output = command_output(
        engine.as_str(),
        &["logs", "--tail", &limit.min(500).to_string(), name],
    )
    .await?;
    Ok(output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let line = line.trim();
            line.chars().take(500).collect::<String>()
        })
        .collect())
}

pub async fn discover_containers(
    engine: ContainerEngine,
) -> Result<Vec<ManagedContainerInventory>, String> {
    ensure_linux()?;
    let output = command_output(
        engine.as_str(),
        &[
            "ps",
            "--all",
            "--filter",
            "label=dev.lumensource.managed=vllm",
            "--format",
            "{{.Names}}\t{{.Label \"dev.lumensource.entry-id\"}}\t{{.Label \"dev.lumensource.model-id\"}}\t{{.Label \"dev.lumensource.served-model\"}}\t{{.Label \"dev.lumensource.port\"}}\t{{.State}}",
        ],
    )
    .await?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() != 6 || fields[0].trim().is_empty() {
                return None;
            }
            Some(ManagedContainerInventory {
                name: fields[0].trim().to_owned(),
                entry_id: fields[1].trim().to_owned(),
                model_id: fields[2].trim().to_owned(),
                served_model_name: fields[3].trim().to_owned(),
                port: fields[4].trim().parse().ok(),
                running: fields[5].trim().eq_ignore_ascii_case("running"),
            })
        })
        .collect())
}

pub async fn delete_caches(engine: ContainerEngine, confirmed: bool) -> Result<(), String> {
    if !confirmed {
        return Err("Confirm cache deletion before removing managed vLLM volumes.".to_owned());
    }
    for volume in [HUGGING_FACE_VOLUME, VLLM_CACHE_VOLUME] {
        run_checked(
            engine,
            &["volume", "rm", volume],
            "remove a managed vLLM cache volume",
        )
        .await?;
    }
    Ok(())
}

pub fn parse_engine(value: &str) -> Option<ContainerEngine> {
    match value {
        "docker" => Some(ContainerEngine::Docker),
        "podman" => Some(ContainerEngine::Podman),
        _ => None,
    }
}

fn ensure_linux() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err("Managed vLLM is supported on Linux NVIDIA hosts only.".to_owned())
    }
}

async fn run_checked(
    engine: ContainerEngine,
    arguments: &[&str],
    action: &str,
) -> Result<(), String> {
    let output = Command::new(engine.as_str())
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("Could not {action}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(sanitized_command_error(action, &output))
    }
}

#[cfg(target_os = "linux")]
async fn command_succeeds(program: &str, arguments: &[&str]) -> bool {
    Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

async fn command_output(program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("Could not run {program}: {error}"))?;
    if !output.status.success() {
        return Err(sanitized_command_error(
            "run the container command",
            &output,
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn sanitized_command_error(action: &str, output: &std::process::Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr)
        .lines()
        .last()
        .unwrap_or("no diagnostic was returned")
        .chars()
        .take(500)
        .collect::<String>();
    format!("Could not {action}: {detail}")
}

pub fn validate_spec(spec: &ManagedVllmSpec) -> Result<(), String> {
    if spec.port == 0
        || spec.model_id.trim().is_empty()
        || spec.served_model_name.trim().is_empty()
        || spec.defaults.pinned_runtime_version != "0.23.0"
        || spec.settings.runtime_management_mode
            != Some(crate::settings::RuntimeManagementMode::Managed)
        || !matches!(
            spec.settings.inference_task,
            Some(ModelInferenceTask::Chat | ModelInferenceTask::Embeddings)
        )
    {
        return Err("The managed vLLM deployment specification is incomplete.".to_owned());
    }
    let model_parts = spec.model_id.split('/').collect::<Vec<_>>();
    let valid_hugging_face_id = model_parts.len() == 2
        && model_parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
        });
    if Path::new(&spec.model_id).is_absolute() || !valid_hugging_face_id {
        return Err(
            "Managed vLLM only accepts a cataloged Hugging Face model identifier.".to_owned(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ManagedVllmSpec {
        ManagedVllmSpec {
            entry_id: "123e4567-e89b-12d3-a456-426614174000".to_owned(),
            model_id: "Qwen/Qwen3-8B".to_owned(),
            served_model_name: "qwen3".to_owned(),
            port: 8_000,
            settings: ModelSettings {
                runtime_management_mode: Some(crate::settings::RuntimeManagementMode::Managed),
                inference_task: Some(ModelInferenceTask::Chat),
                vllm_model_revision: Some("0123456789abcdef".to_owned()),
                ..ModelSettings::default()
            },
            defaults: VllmSettings::default(),
        }
    }

    #[test]
    fn managed_arguments_are_predetermined_and_security_sensitive_flags_are_absent() {
        let spec = spec();
        let arguments = spec.runtime_arguments();
        assert!(validate_spec(&spec).is_ok());
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["--model", "Qwen/Qwen3-8B"]));
        assert!(arguments.contains(&"--disable-fastapi-docs".to_owned()));
        assert!(!arguments
            .iter()
            .any(|argument| argument.contains("trust-remote-code")));
        assert!(!arguments
            .iter()
            .any(|argument| argument.contains("allowed-local-media")));
    }

    #[test]
    fn container_identity_is_stable_and_scoped_to_lumen_source() {
        assert_eq!(
            spec().container_name(),
            "lumensource-vllm-123e4567e89b12d3a4564266"
        );
    }

    #[test]
    fn absolute_local_model_paths_are_rejected() {
        let mut spec = spec();
        spec.model_id = if cfg!(target_os = "windows") {
            "C:\\models\\unsafe".to_owned()
        } else {
            "/models/unsafe".to_owned()
        };
        assert!(validate_spec(&spec).is_err());
    }
}
