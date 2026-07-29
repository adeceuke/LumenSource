use std::collections::BTreeMap;
use std::path::PathBuf;

use reqwest::Url;
use serde::{Deserialize, Serialize};

pub const SETTINGS_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeId {
    #[default]
    Ollama,
    Vllm,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeManagementMode {
    #[default]
    Managed,
    External,
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VllmSettings {}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelSettings {
    pub runtime_management_mode: Option<RuntimeManagementMode>,
    pub endpoint: Option<String>,
    pub context_length: Option<u32>,
    pub keep_alive: Option<String>,
    pub load_on_startup: Option<bool>,
    pub preferred_accelerator: Option<String>,
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
    errors
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
}
