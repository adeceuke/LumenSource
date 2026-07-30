use std::collections::BTreeMap;
use std::path::PathBuf;

use reqwest::Url;
use serde::{Deserialize, Serialize};

pub use crate::runtime_registry::RuntimeId;

pub const SETTINGS_SCHEMA_VERSION: u32 = 5;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeManagementMode {
    #[default]
    Managed,
    External,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelInferenceTask {
    #[default]
    Chat,
    Embeddings,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReasoningLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PerformanceProfile {
    Safe,
    #[default]
    Balanced,
    Fast,
    Custom,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSecretKind {
    VllmApiKey,
    HuggingFaceToken,
}

impl RuntimeSecretKind {
    pub fn service_name(self) -> &'static str {
        match self {
            Self::VllmApiKey => "dev.lumensource.desktop.vllm",
            Self::HuggingFaceToken => "dev.lumensource.desktop.hugging-face",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ApplicationSettings {
    pub schema_version: u32,
    pub default_runtime: RuntimeId,
    pub default_target_id: String,
    pub start_after_install: bool,
    pub auto_start_managed_runtimes: bool,
    pub storage: StorageSettings,
    pub privacy: PrivacySettings,
    pub ollama: OllamaSettings,
    pub vllm: VllmSettings,
}

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            default_runtime: RuntimeId::Ollama,
            default_target_id: "local".to_owned(),
            start_after_install: true,
            auto_start_managed_runtimes: true,
            storage: StorageSettings::default(),
            privacy: PrivacySettings::default(),
            ollama: OllamaSettings::default(),
            vllm: VllmSettings::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StorageSettings {
    pub model_directory: Option<PathBuf>,
    pub cache_directory: Option<PathBuf>,
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            model_directory: default_model_directory(),
            cache_directory: dirs::cache_dir().map(|directory| directory.join("lumen-source")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PrivacySettings {
    pub telemetry_enabled: bool,
    pub lifecycle_log_retention: u32,
    pub confirm_model_deletion: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            telemetry_enabled: false,
            lifecycle_log_retention: 200,
            confirm_model_deletion: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OllamaSettings {
    pub endpoint: String,
    pub executable_path: Option<PathBuf>,
    pub context_length: Option<u32>,
    pub keep_alive: String,
    pub max_loaded_models: u16,
    pub parallel_requests: u16,
    pub max_queued_requests: u32,
    pub gpu_selection: Option<String>,
    pub experimental_vulkan: bool,
    pub bind_address: String,
    pub allowed_origins: Vec<String>,
    pub debug_logging: bool,
}

impl Default for OllamaSettings {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:11434".to_owned(),
            executable_path: None,
            context_length: None,
            keep_alive: "5m".to_owned(),
            max_loaded_models: 1,
            parallel_requests: 1,
            max_queued_requests: 512,
            gpu_selection: None,
            experimental_vulkan: false,
            bind_address: "127.0.0.1:11434".to_owned(),
            allowed_origins: Vec::new(),
            debug_logging: false,
        }
    }
}

impl OllamaSettings {
    pub fn server_environment(
        &self,
        model_directory: Option<&std::path::Path>,
    ) -> BTreeMap<String, String> {
        let mut environment = BTreeMap::from([
            ("OLLAMA_HOST".to_owned(), self.bind_address.clone()),
            ("OLLAMA_KEEP_ALIVE".to_owned(), self.keep_alive.clone()),
            (
                "OLLAMA_MAX_LOADED_MODELS".to_owned(),
                self.max_loaded_models.to_string(),
            ),
            (
                "OLLAMA_NUM_PARALLEL".to_owned(),
                self.parallel_requests.to_string(),
            ),
            (
                "OLLAMA_MAX_QUEUE".to_owned(),
                self.max_queued_requests.to_string(),
            ),
            ("OLLAMA_DEBUG".to_owned(), self.debug_logging.to_string()),
        ]);
        if let Some(context_length) = self.context_length {
            environment.insert(
                "OLLAMA_CONTEXT_LENGTH".to_owned(),
                context_length.to_string(),
            );
        }
        if let Some(path) = model_directory {
            environment.insert("OLLAMA_MODELS".to_owned(), path.display().to_string());
        }
        if let Some(gpu_selection) = self
            .gpu_selection
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            environment.insert("CUDA_VISIBLE_DEVICES".to_owned(), gpu_selection.to_owned());
            environment.insert("ROCR_VISIBLE_DEVICES".to_owned(), gpu_selection.to_owned());
            environment.insert(
                "GGML_VK_VISIBLE_DEVICES".to_owned(),
                gpu_selection.to_owned(),
            );
        }
        if self.experimental_vulkan {
            environment.insert("OLLAMA_VULKAN".to_owned(), "1".to_owned());
        }
        if !self.allowed_origins.is_empty() {
            environment.insert("OLLAMA_ORIGINS".to_owned(), self.allowed_origins.join(","));
        }
        environment
    }

    pub fn exposes_network(&self) -> bool {
        let address = self.bind_address.trim().to_ascii_lowercase();
        !(address.starts_with("127.")
            || address.starts_with("localhost:")
            || address.starts_with("[::1]:")
            || address == "::1")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VllmSettings {
    pub hugging_face_cache_directory: Option<PathBuf>,
    pub gpu_selection: Option<String>,
    pub gpu_memory_utilization: f32,
    pub max_context_length: Option<u32>,
    pub max_concurrent_sequences: u32,
    pub prefix_caching: bool,
    pub weight_dtype: String,
    pub quantization: Option<String>,
    pub kv_cache_dtype: String,
    pub cpu_offload_gib: f32,
    pub tensor_parallel_size: u16,
    pub pipeline_parallel_size: u16,
    pub bind_address: String,
    pub managed_port_start: u16,
    pub managed_port_end: u16,
    pub pinned_runtime_version: String,
}

impl Default for VllmSettings {
    fn default() -> Self {
        Self {
            hugging_face_cache_directory: dirs::cache_dir()
                .map(|directory| directory.join("huggingface")),
            gpu_selection: None,
            gpu_memory_utilization: 0.9,
            max_context_length: None,
            max_concurrent_sequences: 256,
            prefix_caching: true,
            weight_dtype: "auto".to_owned(),
            quantization: None,
            kv_cache_dtype: "auto".to_owned(),
            cpu_offload_gib: 0.0,
            tensor_parallel_size: 1,
            pipeline_parallel_size: 1,
            bind_address: "127.0.0.1".to_owned(),
            managed_port_start: 8_000,
            managed_port_end: 8_099,
            pinned_runtime_version: "0.23.0".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelSettings {
    pub performance_profile: Option<PerformanceProfile>,
    pub runtime_management_mode: Option<RuntimeManagementMode>,
    pub inference_task: Option<ModelInferenceTask>,
    pub endpoint: Option<String>,
    pub verify_tls: bool,
    pub connection_timeout_seconds: u16,
    pub request_timeout_seconds: u16,
    pub system_prompt: Option<String>,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub repetition_penalty: Option<f32>,
    pub seed: Option<i64>,
    pub stop_sequences: Vec<String>,
    pub structured_output: Option<bool>,
    pub reasoning_level: Option<ReasoningLevel>,
    pub context_length: Option<u32>,
    pub keep_alive: Option<String>,
    pub load_on_startup: Option<bool>,
    pub preferred_accelerator: Option<String>,
    pub ollama_derived_model_name: Option<String>,
    pub ollama_persistent_parameters: bool,
    pub vllm_model_revision: Option<String>,
    pub vllm_tokenizer_revision: Option<String>,
    pub vllm_served_model_name: Option<String>,
    pub vllm_task: Option<String>,
    pub vllm_runner: Option<String>,
    pub vllm_weight_dtype: Option<String>,
    pub vllm_quantization: Option<String>,
    pub vllm_gpu_memory_utilization: Option<f32>,
    pub vllm_max_concurrent_sequences: Option<u32>,
    pub vllm_prefix_caching: Option<bool>,
    pub vllm_kv_cache_dtype: Option<String>,
    pub vllm_cpu_offload_gib: Option<f32>,
    pub vllm_tensor_parallel_size: Option<u16>,
    pub vllm_pipeline_parallel_size: Option<u16>,
    pub managed_container_engine: Option<String>,
    pub managed_container_name: Option<String>,
    pub managed_port: Option<u16>,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            performance_profile: None,
            runtime_management_mode: None,
            inference_task: None,
            endpoint: None,
            verify_tls: true,
            connection_timeout_seconds: 5,
            request_timeout_seconds: 120,
            system_prompt: None,
            temperature: None,
            max_output_tokens: None,
            top_p: None,
            top_k: None,
            min_p: None,
            repetition_penalty: None,
            seed: None,
            stop_sequences: Vec::new(),
            structured_output: None,
            reasoning_level: None,
            context_length: None,
            keep_alive: None,
            load_on_startup: None,
            preferred_accelerator: None,
            ollama_derived_model_name: None,
            ollama_persistent_parameters: false,
            vllm_model_revision: None,
            vllm_tokenizer_revision: None,
            vllm_served_model_name: None,
            vllm_task: None,
            vllm_runner: None,
            vllm_weight_dtype: None,
            vllm_quantization: None,
            vllm_gpu_memory_utilization: None,
            vllm_max_concurrent_sequences: None,
            vllm_prefix_caching: None,
            vllm_kv_cache_dtype: None,
            vllm_cpu_offload_gib: None,
            vllm_tensor_parallel_size: None,
            vllm_pipeline_parallel_size: None,
            managed_container_engine: None,
            managed_container_name: None,
            managed_port: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsValidationError {
    pub field: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSaveReport {
    pub settings: ApplicationSettings,
    pub runtime_restart_required: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaConnectionReport {
    pub healthy: bool,
    pub endpoint: String,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExternalVllmConfig {
    pub endpoint: String,
    pub served_model: String,
    pub inference_task: ModelInferenceTask,
    pub verify_tls: bool,
    pub connection_timeout_seconds: u16,
    pub request_timeout_seconds: u16,
}

impl Default for ExternalVllmConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8000".to_owned(),
            served_model: String::new(),
            inference_task: ModelInferenceTask::Chat,
            verify_tls: true,
            connection_timeout_seconds: 5,
            request_timeout_seconds: 120,
        }
    }
}

impl ExternalVllmConfig {
    pub fn model_settings(&self) -> ModelSettings {
        ModelSettings {
            runtime_management_mode: Some(RuntimeManagementMode::External),
            inference_task: Some(self.inference_task),
            endpoint: Some(self.endpoint.clone()),
            verify_tls: self.verify_tls,
            connection_timeout_seconds: self.connection_timeout_seconds,
            request_timeout_seconds: self.request_timeout_seconds,
            ..ModelSettings::default()
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VllmConnectionReport {
    pub healthy: bool,
    pub authenticated: bool,
    pub endpoint: String,
    pub models: Vec<String>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettingsSaveReport {
    pub model: crate::bridge_types::PersistedModelEntry,
    pub restart_required: bool,
    pub restarted: bool,
    pub message: String,
}

pub fn migrate_settings(mut settings: ApplicationSettings) -> ApplicationSettings {
    let defaults = ApplicationSettings::default();
    let previous_version = settings.schema_version;
    if previous_version == 1 {
        let legacy_model_directory =
            dirs::data_local_dir().map(|directory| directory.join("lumen-source").join("models"));
        if settings.storage.model_directory == legacy_model_directory {
            settings.storage.model_directory = default_model_directory();
        }
    }
    if settings.schema_version < SETTINGS_SCHEMA_VERSION {
        settings.schema_version = SETTINGS_SCHEMA_VERSION;
    }
    if settings.storage.model_directory.is_none() {
        settings.storage.model_directory = defaults.storage.model_directory;
    }
    if settings.storage.cache_directory.is_none() {
        settings.storage.cache_directory = defaults.storage.cache_directory;
    }
    settings
}

fn default_model_directory() -> Option<PathBuf> {
    std::env::var_os("OLLAMA_MODELS")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|directory| directory.join(".ollama").join("models")))
}

pub fn validate_settings(settings: &ApplicationSettings) -> Vec<SettingsValidationError> {
    let mut errors = Vec::new();
    if settings.schema_version != SETTINGS_SCHEMA_VERSION {
        push_error(
            &mut errors,
            "schemaVersion",
            format!(
                "Unsupported settings version {}. This app supports version {SETTINGS_SCHEMA_VERSION}.",
                settings.schema_version
            ),
        );
    }
    validate_http_url(&mut errors, "ollama.endpoint", &settings.ollama.endpoint);
    if settings.default_target_id.trim().is_empty() {
        push_error(
            &mut errors,
            "defaultTargetId",
            "Choose a default installation target.",
        );
    }
    if let Some(context_length) = settings.ollama.context_length {
        if !(256..=1_048_576).contains(&context_length) {
            push_error(
                &mut errors,
                "ollama.contextLength",
                "Context length must be between 256 and 1,048,576 tokens.",
            );
        }
    }
    if !valid_duration(&settings.ollama.keep_alive) {
        push_error(
            &mut errors,
            "ollama.keepAlive",
            "Use a duration such as 5m, 30s, 2h, 0, or -1.",
        );
    }
    validate_range(
        &mut errors,
        "ollama.maxLoadedModels",
        settings.ollama.max_loaded_models.into(),
        1,
        128,
    );
    validate_range(
        &mut errors,
        "ollama.parallelRequests",
        settings.ollama.parallel_requests.into(),
        1,
        128,
    );
    validate_range(
        &mut errors,
        "ollama.maxQueuedRequests",
        settings.ollama.max_queued_requests.into(),
        1,
        100_000,
    );
    if !valid_bind_address(&settings.ollama.bind_address) {
        push_error(
            &mut errors,
            "ollama.bindAddress",
            "Use a host and port, for example 127.0.0.1:11434.",
        );
    }
    if let Some(selection) = &settings.ollama.gpu_selection {
        if !selection.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, ',' | '-' | '_' | '.' | ':' | ' ')
        }) {
            push_error(
                &mut errors,
                "ollama.gpuSelection",
                "GPU selection contains unsupported characters.",
            );
        }
    }
    for (index, origin) in settings.ollama.allowed_origins.iter().enumerate() {
        if origin != "*" && Url::parse(origin).is_err() {
            push_error(
                &mut errors,
                format!("ollama.allowedOrigins.{index}"),
                "Each allowed origin must be a URL or *.",
            );
        }
    }
    validate_range(
        &mut errors,
        "privacy.lifecycleLogRetention",
        settings.privacy.lifecycle_log_retention.into(),
        0,
        10_000,
    );
    if !(0.1..=1.0).contains(&settings.vllm.gpu_memory_utilization) {
        push_error(
            &mut errors,
            "vllm.gpuMemoryUtilization",
            "GPU memory utilization must be between 0.1 and 1.0.",
        );
    }
    validate_range(
        &mut errors,
        "vllm.maxConcurrentSequences",
        settings.vllm.max_concurrent_sequences.into(),
        1,
        65_536,
    );
    validate_range(
        &mut errors,
        "vllm.tensorParallelSize",
        settings.vllm.tensor_parallel_size.into(),
        1,
        256,
    );
    validate_range(
        &mut errors,
        "vllm.pipelineParallelSize",
        settings.vllm.pipeline_parallel_size.into(),
        1,
        256,
    );
    if settings.vllm.managed_port_start == 0
        || settings.vllm.managed_port_end < settings.vllm.managed_port_start
    {
        push_error(
            &mut errors,
            "vllm.managedPortStart",
            "The managed vLLM port range is invalid.",
        );
    }
    if settings.vllm.pinned_runtime_version != "0.23.0" {
        push_error(
            &mut errors,
            "vllm.pinnedRuntimeVersion",
            "This version of Lumen Source supports managed-vLLM defaults for vLLM 0.23.0.",
        );
    }
    if !matches!(
        settings.vllm.weight_dtype.as_str(),
        "auto" | "half" | "float16" | "bfloat16" | "float" | "float32"
    ) {
        push_error(
            &mut errors,
            "vllm.weightDtype",
            "Choose a data type supported by the pinned vLLM version.",
        );
    }
    if !matches!(
        settings.vllm.kv_cache_dtype.as_str(),
        "auto" | "fp8" | "fp8_e4m3" | "fp8_e5m2" | "fp8_inc"
    ) {
        push_error(
            &mut errors,
            "vllm.kvCacheDtype",
            "Choose a KV-cache data type supported by the pinned vLLM version.",
        );
    }
    errors
}

pub fn validate_external_model_settings(settings: &ModelSettings) -> Vec<SettingsValidationError> {
    let mut errors = Vec::new();
    match settings.endpoint.as_deref() {
        Some(endpoint) => validate_http_url(&mut errors, "endpoint", endpoint),
        None => push_error(&mut errors, "endpoint", "Enter the vLLM endpoint URL."),
    }
    validate_range(
        &mut errors,
        "connectionTimeoutSeconds",
        settings.connection_timeout_seconds.into(),
        1,
        300,
    );
    validate_range(
        &mut errors,
        "requestTimeoutSeconds",
        settings.request_timeout_seconds.into(),
        1,
        3_600,
    );
    errors
}

pub fn validate_external_vllm_config(
    config: &ExternalVllmConfig,
    require_model: bool,
) -> Vec<SettingsValidationError> {
    let mut errors = validate_external_model_settings(&config.model_settings());
    if require_model && config.served_model.trim().is_empty() {
        push_error(
            &mut errors,
            "servedModel",
            "Choose the model served by this vLLM endpoint.",
        );
    }
    errors
}

pub fn validate_model_settings(
    settings: &ModelSettings,
    maximum_context_length: Option<u32>,
) -> Vec<SettingsValidationError> {
    let mut errors = Vec::new();
    validate_optional_float(&mut errors, "temperature", settings.temperature, 0.0, 2.0);
    validate_optional_float(&mut errors, "topP", settings.top_p, 0.0, 1.0);
    validate_optional_float(&mut errors, "minP", settings.min_p, 0.0, 1.0);
    validate_optional_float(
        &mut errors,
        "repetitionPenalty",
        settings.repetition_penalty,
        0.0,
        2.0,
    );
    validate_optional_float(
        &mut errors,
        "vllmGpuMemoryUtilization",
        settings.vllm_gpu_memory_utilization,
        0.1,
        1.0,
    );
    if settings.max_output_tokens.is_some_and(|value| value == 0) {
        push_error(
            &mut errors,
            "maxOutputTokens",
            "Maximum output tokens must be greater than zero.",
        );
    }
    if settings.top_k.is_some_and(|value| value == 0) {
        push_error(&mut errors, "topK", "Top-k must be greater than zero.");
    }
    if let Some(context_length) = settings.context_length {
        let maximum = maximum_context_length.unwrap_or(1_048_576);
        if context_length < 256 || context_length > maximum {
            push_error(
                &mut errors,
                "contextLength",
                format!("Context length must be between 256 and {maximum} tokens."),
            );
        }
    }
    if let Some(keep_alive) = settings.keep_alive.as_deref() {
        if !valid_duration(keep_alive) {
            push_error(
                &mut errors,
                "keepAlive",
                "Use a duration such as 5m, 30s, 2h, 0, or -1.",
            );
        }
    }
    if settings.stop_sequences.len() > 16
        || settings
            .stop_sequences
            .iter()
            .any(|sequence| sequence.is_empty() || sequence.len() > 256)
    {
        push_error(
            &mut errors,
            "stopSequences",
            "Provide at most 16 non-empty stop sequences of at most 256 characters.",
        );
    }
    if settings.ollama_persistent_parameters
        && settings
            .ollama_derived_model_name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
    {
        push_error(
            &mut errors,
            "ollamaDerivedModelName",
            "A derived model name is required for persistent Ollama parameters.",
        );
    }
    for (field, value) in [
        (
            "ollamaDerivedModelName",
            settings.ollama_derived_model_name.as_deref(),
        ),
        ("vllmModelRevision", settings.vllm_model_revision.as_deref()),
        (
            "vllmTokenizerRevision",
            settings.vllm_tokenizer_revision.as_deref(),
        ),
        (
            "vllmServedModelName",
            settings.vllm_served_model_name.as_deref(),
        ),
        ("vllmTask", settings.vllm_task.as_deref()),
        ("vllmRunner", settings.vllm_runner.as_deref()),
        ("vllmWeightDtype", settings.vllm_weight_dtype.as_deref()),
        ("vllmQuantization", settings.vllm_quantization.as_deref()),
        ("vllmKvCacheDtype", settings.vllm_kv_cache_dtype.as_deref()),
        (
            "preferredAccelerator",
            settings.preferred_accelerator.as_deref(),
        ),
    ] {
        if value.is_some_and(|value| {
            value.len() > 160 || value.contains(['\n', '\r', '\0']) || value.starts_with('-')
        }) {
            push_error(&mut errors, field, "Value contains unsupported characters.");
        }
    }
    if settings.vllm_weight_dtype.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "auto" | "half" | "float16" | "bfloat16" | "float" | "float32"
        )
    }) {
        push_error(
            &mut errors,
            "vllmWeightDtype",
            "Choose a data type supported by the pinned vLLM version.",
        );
    }
    if settings
        .vllm_kv_cache_dtype
        .as_deref()
        .is_some_and(|value| !matches!(value, "auto" | "fp8" | "fp8_e4m3" | "fp8_e5m2" | "fp8_inc"))
    {
        push_error(
            &mut errors,
            "vllmKvCacheDtype",
            "Choose a KV-cache data type supported by the pinned vLLM version.",
        );
    }
    for (field, value) in [
        ("vllmTask", settings.vllm_task.as_deref()),
        ("vllmRunner", settings.vllm_runner.as_deref()),
    ] {
        if value.is_some_and(|value| {
            !matches!(
                value,
                "auto" | "generate" | "draft" | "pooling" | "transcription"
            )
        }) {
            push_error(
                &mut errors,
                field,
                "Choose a task or runner supported by the pinned vLLM version.",
            );
        }
    }
    if settings
        .vllm_max_concurrent_sequences
        .is_some_and(|value| value == 0 || value > 65_536)
    {
        push_error(
            &mut errors,
            "vllmMaxConcurrentSequences",
            "Concurrent sequences must be between 1 and 65,536.",
        );
    }
    for (field, value) in [
        ("vllmTensorParallelSize", settings.vllm_tensor_parallel_size),
        (
            "vllmPipelineParallelSize",
            settings.vllm_pipeline_parallel_size,
        ),
    ] {
        if value.is_some_and(|value| value == 0 || value > 256) {
            push_error(
                &mut errors,
                field,
                "Parallel size must be between 1 and 256.",
            );
        }
    }
    errors
}

fn validate_optional_float(
    errors: &mut Vec<SettingsValidationError>,
    field: &str,
    value: Option<f32>,
    minimum: f32,
    maximum: f32,
) {
    if value.is_some_and(|value| !value.is_finite() || !(minimum..=maximum).contains(&value)) {
        push_error(
            errors,
            field,
            format!("Value must be between {minimum} and {maximum}."),
        );
    }
}

fn validate_http_url(errors: &mut Vec<SettingsValidationError>, field: &str, value: &str) {
    match Url::parse(value) {
        Ok(url)
            if matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none() => {}
        _ => push_error(
            errors,
            field,
            "Use an http or https URL without credentials, a query, or a fragment.",
        ),
    }
}

fn valid_duration(value: &str) -> bool {
    let value = value.trim();
    if matches!(value, "-1" | "0") {
        return true;
    }
    const UNITS: [&str; 6] = ["ns", "us", "ms", "s", "m", "h"];
    UNITS.iter().any(|unit| {
        value
            .strip_suffix(unit)
            .is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
    })
}

fn valid_bind_address(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return false;
    }
    let Ok(url) = Url::parse(&format!("http://{value}")) else {
        return false;
    };
    url.host_str().is_some() && url.port().is_some()
}

fn validate_range(
    errors: &mut Vec<SettingsValidationError>,
    field: &str,
    value: u64,
    minimum: u64,
    maximum: u64,
) {
    if !(minimum..=maximum).contains(&value) {
        push_error(
            errors,
            field,
            format!("Value must be between {minimum} and {maximum}."),
        );
    }
}

fn push_error(
    errors: &mut Vec<SettingsValidationError>,
    field: impl Into<String>,
    message: impl Into<String>,
) {
    errors.push(SettingsValidationError {
        field: field.into(),
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_settings_use_ollama_and_safe_local_defaults() {
        let settings = ApplicationSettings::default();
        assert_eq!(settings.default_runtime, RuntimeId::Ollama);
        assert!(!settings.ollama.exposes_network());
        assert!(validate_settings(&settings).is_empty());
    }

    #[test]
    fn old_and_partial_settings_deserialize_with_current_defaults() {
        let Ok(settings): Result<ApplicationSettings, _> =
            serde_json::from_str(r#"{"schemaVersion":0,"startAfterInstall":false}"#)
        else {
            panic!("partial settings should deserialize");
        };
        let settings = migrate_settings(settings);
        assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(settings.default_runtime, RuntimeId::Ollama);
        assert!(!settings.start_after_install);
    }

    #[test]
    fn schema_one_storage_default_migrates_to_the_active_ollama_directory() {
        let settings = ApplicationSettings {
            schema_version: 1,
            storage: StorageSettings {
                model_directory: dirs::data_local_dir()
                    .map(|directory| directory.join("lumen-source").join("models")),
                ..StorageSettings::default()
            },
            ..ApplicationSettings::default()
        };

        let settings = migrate_settings(settings);

        assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(settings.storage.model_directory, default_model_directory());
    }

    #[test]
    fn rejects_network_and_limit_mistakes() {
        let mut settings = ApplicationSettings::default();
        settings.ollama.endpoint = "file:///tmp/ollama".to_owned();
        settings.ollama.keep_alive = "forever".to_owned();
        settings.ollama.parallel_requests = 0;
        settings.ollama.bind_address = "localhost".to_owned();
        let fields = validate_settings(&settings)
            .into_iter()
            .map(|error| error.field)
            .collect::<Vec<_>>();
        assert!(fields.contains(&"ollama.endpoint".to_owned()));
        assert!(fields.contains(&"ollama.keepAlive".to_owned()));
        assert!(fields.contains(&"ollama.parallelRequests".to_owned()));
        assert!(fields.contains(&"ollama.bindAddress".to_owned()));
    }

    #[test]
    fn managed_environment_does_not_include_automatic_context() {
        let settings = OllamaSettings::default();
        let environment = settings.server_environment(None);
        assert!(!environment.contains_key("OLLAMA_CONTEXT_LENGTH"));
        assert_eq!(
            environment.get("OLLAMA_HOST").map(String::as_str),
            Some("127.0.0.1:11434")
        );
    }

    #[test]
    fn external_vllm_settings_reject_credentials_in_urls_and_invalid_limits() {
        let config = ExternalVllmConfig {
            endpoint: "https://user:secret@example.test:8000".to_owned(),
            served_model: String::new(),
            connection_timeout_seconds: 0,
            request_timeout_seconds: 3_601,
            ..ExternalVllmConfig::default()
        };
        let fields = validate_external_vllm_config(&config, true)
            .into_iter()
            .map(|error| error.field)
            .collect::<Vec<_>>();

        assert!(fields.contains(&"endpoint".to_owned()));
        assert!(fields.contains(&"servedModel".to_owned()));
        assert!(fields.contains(&"connectionTimeoutSeconds".to_owned()));
        assert!(fields.contains(&"requestTimeoutSeconds".to_owned()));
    }

    #[test]
    fn managed_vllm_defaults_are_pinned_and_security_constrained() {
        let settings = VllmSettings::default();

        assert_eq!(settings.pinned_runtime_version, "0.23.0");
        assert_eq!(settings.bind_address, "127.0.0.1");
        assert!((settings.gpu_memory_utilization - 0.9).abs() < f32::EPSILON);
        assert!(validate_settings(&ApplicationSettings::default()).is_empty());
    }

    #[test]
    fn model_settings_validate_catalog_limits_and_persistent_identity() {
        let settings = ModelSettings {
            context_length: Some(65_536),
            ollama_persistent_parameters: true,
            ..ModelSettings::default()
        };
        let fields = validate_model_settings(&settings, Some(32_768))
            .into_iter()
            .map(|error| error.field)
            .collect::<Vec<_>>();

        assert!(fields.contains(&"contextLength".to_owned()));
        assert!(fields.contains(&"ollamaDerivedModelName".to_owned()));
    }
}
