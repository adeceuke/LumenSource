use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::CatalogError;

pub const CURRENT_SCHEMA_VERSION: &str = "1";
pub const CURRENT_MODEL_LIST_SCHEMA_VERSION: &str = "2";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: String,
    pub catalog_version: String,
    pub published_at: String,
    pub runtimes: Vec<RuntimeEntry>,
    pub models: Vec<ModelEntry>,
}

impl Catalog {
    pub fn from_slice(bytes: &[u8]) -> Result<Self, CatalogError> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let catalog = if value.get("generated_at").is_some() {
            let model_list: ModelList = serde_json::from_value(value)?;
            if model_list.schema_version != CURRENT_MODEL_LIST_SCHEMA_VERSION {
                return Err(CatalogError::UnsupportedSchemaVersion {
                    expected: CURRENT_MODEL_LIST_SCHEMA_VERSION,
                    found: model_list.schema_version,
                });
            }
            ModelList::into_catalog(model_list)
        } else {
            serde_json::from_value(value)?
        };
        catalog.validate_schema_version()?;
        Ok(catalog)
    }

    pub fn validate_schema_version(&self) -> Result<(), CatalogError> {
        if self.schema_version == CURRENT_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(CatalogError::UnsupportedSchemaVersion {
                expected: CURRENT_SCHEMA_VERSION,
                found: self.schema_version.clone(),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeEntry {
    pub id: String,
    pub display_name: String,
    pub platforms: Vec<Platform>,
    pub install: Install,
    pub default_endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub strategy: InstallStrategy,
    pub urls_by_platform: BTreeMap<Platform, String>,
    pub sha256_by_platform: BTreeMap<Platform, String>,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstallStrategy(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub description: String,
    pub capabilities: Vec<String>,
    pub languages: Vec<String>,
    pub license: License,
    pub use_cases: Vec<String>,
    pub variants: Vec<ModelVariant>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spdx: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default = "unknown_legal_value")]
    pub classification: String,
    #[serde(default = "unknown_legal_value")]
    pub commercial_use: String,
    #[serde(default = "unknown_legal_value")]
    pub redistribution: String,
    #[serde(default = "unknown_legal_value")]
    pub derivatives: String,
    #[serde(default)]
    pub requires_user_acceptance: bool,
    #[serde(default = "unknown_legal_value")]
    pub attribution: String,
    #[serde(default = "unknown_legal_value")]
    pub license_text: String,
    #[serde(default = "unknown_legal_value")]
    pub notice: String,
    #[serde(default = "informational_ui_notice")]
    pub ui_notice: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub obligations: Vec<String>,
    #[serde(default)]
    pub restrictions: Vec<String>,
    #[serde(default)]
    pub geographic_restrictions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_policy_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<String>,
}

fn unknown_legal_value() -> String {
    "unknown".to_owned()
}

fn informational_ui_notice() -> String {
    "informational".to_owned()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalEvaluationSource {
    pub url: String,
    pub retrieved_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalEvaluation {
    pub publisher: String,
    pub leaderboard_name: String,
    pub source_model_name: String,
    pub overall_tier: OverallTier,
    pub notes: String,
    pub source: ExternalEvaluationSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverallTier {
    S,
    A,
    B,
    C,
    D,
}

impl OverallTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::S => "S",
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelVariant {
    pub id: String,
    pub runtime: String,
    pub runtime_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ollama_model_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hugging_face_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokenizer_revision: Option<String>,
    #[serde(default)]
    pub gated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default)]
    pub runtime_compatibility: Vec<String>,
    pub parameters_b: f64,
    pub quantization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_item_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_size_bytes: Option<u64>,
    pub requirements: Requirements,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance_hint: Option<PerformanceHint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_evaluations: Vec<ExternalEvaluation>,
    pub recommended_for: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelList {
    #[serde(rename = "$schema")]
    schema: Option<String>,
    schema_version: String,
    catalog_version: String,
    generated_at: String,
    generator: ModelListGenerator,
    models: Vec<ModelListModel>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelListGenerator {
    name: String,
    version: String,
    homepage_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelListModel {
    id: String,
    display_name: String,
    provider: String,
    family: Option<String>,
    description: String,
    homepage_url: String,
    release_date: Option<String>,
    knowledge_cutoff: Option<String>,
    license: License,
    capabilities: Vec<String>,
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
    use_cases: Vec<ModelListUseCase>,
    languages: Vec<String>,
    #[serde(default)]
    limitations: Vec<String>,
    variants: Vec<ModelListVariant>,
    sources: Vec<ModelListSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelListUseCase {
    id: String,
    suitability: String,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelListVariant {
    id: String,
    display_name: String,
    runtime: ModelListRuntime,
    parameters_billion: f64,
    quantization: Option<String>,
    context_window_tokens: u32,
    model_size_bytes: u64,
    download_item_count: u32,
    size_is_estimate: bool,
    requirements: ModelListRequirements,
    recommended_for: Vec<String>,
    #[serde(default)]
    benchmarks: Vec<ModelListBenchmark>,
    #[serde(default)]
    external_evaluations: Vec<ExternalEvaluation>,
    sources: Vec<ModelListSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelListRuntime {
    engine: String,
    model_ref: String,
    digest: Option<String>,
    #[serde(default)]
    hugging_face_model_id: Option<String>,
    #[serde(default)]
    model_revision: Option<String>,
    #[serde(default)]
    tokenizer_revision: Option<String>,
    #[serde(default)]
    gated: bool,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    runner: Option<String>,
    #[serde(default)]
    compatibility: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelListRequirements {
    minimum_system_ram_bytes: u64,
    recommended_system_ram_bytes: u64,
    minimum_vram_bytes: Option<u64>,
    recommended_vram_bytes: Option<u64>,
    minimum_free_storage_bytes: u64,
    supported_os: Vec<OperatingSystem>,
    supported_architectures: Vec<String>,
    accelerators: Vec<Accelerator>,
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelListBenchmark {
    tokens_per_second: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelListSource {
    url: String,
    retrieved_at: String,
    notes: Option<String>,
}

impl ModelList {
    fn into_catalog(model_list: Self) -> Catalog {
        let _producer_metadata = (
            model_list.schema,
            model_list.generator.name,
            model_list.generator.version,
            model_list.generator.homepage_url,
        );
        Catalog {
            schema_version: CURRENT_SCHEMA_VERSION.to_owned(),
            catalog_version: model_list.catalog_version,
            published_at: model_list.generated_at,
            runtimes: vec![ollama_runtime()],
            models: model_list
                .models
                .into_iter()
                .map(ModelListModel::into_model)
                .collect(),
        }
    }
}

impl ModelListModel {
    fn into_model(self) -> ModelEntry {
        let _model_metadata = (
            self.family,
            self.homepage_url,
            self.release_date,
            self.knowledge_cutoff,
            self.input_modalities,
            self.output_modalities,
            self.limitations,
        );
        let _sources = self
            .sources
            .into_iter()
            .map(|source| (source.url, source.retrieved_at, source.notes))
            .collect::<Vec<_>>();
        ModelEntry {
            id: self.id,
            display_name: self.display_name,
            provider: Some(self.provider),
            description: self.description,
            capabilities: self.capabilities,
            languages: self.languages,
            license: self.license,
            use_cases: self
                .use_cases
                .into_iter()
                .filter(|use_case| use_case.suitability != "unsuitable")
                .map(|use_case| {
                    let _notes = use_case.notes;
                    use_case.id
                })
                .collect(),
            variants: self
                .variants
                .into_iter()
                .map(ModelListVariant::into_variant)
                .collect(),
        }
    }
}

impl ModelListVariant {
    fn into_variant(self) -> ModelVariant {
        let _variant_metadata = (self.display_name, self.size_is_estimate);
        let performance_hint = self.benchmarks.first().map(|benchmark| PerformanceHint {
            tokens_per_sec_estimate: Some(benchmark.tokens_per_second),
            notes: Some(
                "Measured benchmark from the model list; see its attributed source.".to_owned(),
            ),
        });
        let _sources = self
            .sources
            .into_iter()
            .map(|source| (source.url, source.retrieved_at, source.notes))
            .collect::<Vec<_>>();
        let requirements = self.requirements;
        let _requirements_metadata = (requirements.supported_architectures, requirements.notes);
        let ollama_model_ref =
            (self.runtime.engine == "ollama").then(|| self.runtime.model_ref.clone());
        ModelVariant {
            id: self.id,
            runtime: self.runtime.engine,
            runtime_ref: self.runtime.model_ref,
            ollama_model_ref,
            hugging_face_model_id: self.runtime.hugging_face_model_id,
            model_revision: self.runtime.model_revision,
            tokenizer_revision: self.runtime.tokenizer_revision,
            gated: self.runtime.gated,
            task: self.runtime.task,
            runner: self.runtime.runner,
            runtime_compatibility: self.runtime.compatibility,
            parameters_b: self.parameters_billion,
            quantization: self.quantization,
            context_window_tokens: Some(self.context_window_tokens),
            runtime_digest: self.runtime.digest,
            download_item_count: Some(self.download_item_count),
            download_size_bytes: Some(self.model_size_bytes),
            requirements: Requirements {
                min_ram_gb: bytes_to_gib(requirements.minimum_system_ram_bytes),
                recommended_ram_gb: Some(bytes_to_gib(requirements.recommended_system_ram_bytes)),
                min_vram_gb: requirements.minimum_vram_bytes.map(bytes_to_gib),
                recommended_vram_gb: requirements.recommended_vram_bytes.map(bytes_to_gib),
                min_storage_gb: bytes_to_gib(requirements.minimum_free_storage_bytes),
                os: Some(requirements.supported_os),
                accelerators: requirements.accelerators,
            },
            artifact: Some(Artifact {
                url: None,
                sha256: None,
                size_bytes: Some(self.model_size_bytes),
            }),
            performance_hint,
            external_evaluations: self.external_evaluations,
            recommended_for: self.recommended_for,
        }
    }
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn ollama_runtime() -> RuntimeEntry {
    RuntimeEntry {
        id: "ollama".to_owned(),
        display_name: "Ollama".to_owned(),
        platforms: vec![
            Platform::LinuxX86_64,
            Platform::LinuxAarch64,
            Platform::DarwinArm64,
            Platform::WindowsX86_64,
        ],
        install: Install {
            strategy: InstallStrategy("archive".to_owned()),
            urls_by_platform: BTreeMap::from([
                (
                    Platform::LinuxX86_64,
                    "https://github.com/ollama/ollama/releases/download/v0.32.1/ollama-linux-amd64.tar.zst"
                        .to_owned(),
                ),
                (
                    Platform::LinuxAarch64,
                    "https://github.com/ollama/ollama/releases/download/v0.32.1/ollama-linux-arm64.tar.zst"
                        .to_owned(),
                ),
                (
                    Platform::WindowsX86_64,
                    "https://github.com/ollama/ollama/releases/download/v0.32.1/ollama-windows-amd64.zip"
                        .to_owned(),
                ),
            ]),
            sha256_by_platform: BTreeMap::from([
                (
                    Platform::LinuxX86_64,
                    "83b1f22841eb7f6c4900c6797f960ebaa09466874442ea5b8ae3da6980d3914c"
                        .to_owned(),
                ),
                (
                    Platform::LinuxAarch64,
                    "20fb8d14694f73b97dc41519e27ef06166236207e7efe793f1698a43722215f2"
                        .to_owned(),
                ),
                (
                    Platform::WindowsX86_64,
                    "d5abdc21b64ee928d3c92880ac22da5e5b0a46b8b07179791dd8c711b35f8397"
                        .to_owned(),
                ),
            ]),
            version: "0.32.1".to_owned(),
        },
        default_endpoint: "http://127.0.0.1:11434".to_owned(),
        notes: Some(
            "Ollama can be downloaded as a verified standalone runtime after explicit user consent."
                .to_owned(),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirements {
    pub min_ram_gb: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_ram_gb: Option<f64>,
    pub min_vram_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_vram_gb: Option<f64>,
    pub min_storage_gb: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Vec<OperatingSystem>>,
    pub accelerators: Vec<Accelerator>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_per_sec_estimate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "linux-x86_64")]
    LinuxX86_64,
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
    #[serde(rename = "darwin-arm64")]
    DarwinArm64,
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatingSystem {
    Linux,
    Darwin,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accelerator {
    Cuda,
    Metal,
    Rocm,
    Cpu,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const VALID: &[u8] = include_bytes!("../../../catalog/fixtures/catalog.v1.valid.json");
    const MODEL_LIST: &[u8] = include_bytes!("../../../catalog/model-list.json");
    const INVALID_SCHEMA: &[u8] =
        include_bytes!("../../../catalog/fixtures/catalog.invalid-schema.json");
    const INVALID_SHAPE: &[u8] =
        include_bytes!("../../../catalog/fixtures/catalog.invalid-shape.json");

    #[test]
    fn parses_realistic_catalog_fixture() {
        let catalog = Catalog::from_slice(VALID).unwrap();

        assert_eq!(catalog.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(catalog.runtimes[0].id, "ollama");
        assert_eq!(
            catalog.models[0].variants[0].runtime_ref,
            "qwen2.5-coder:14b"
        );
    }

    #[test]
    fn parses_generated_model_list_as_a_catalog() {
        let catalog = Catalog::from_slice(MODEL_LIST).unwrap();

        assert_eq!(catalog.catalog_version, "2026.08.01.3");
        assert_eq!(catalog.models.len(), 36);
        assert_eq!(
            catalog
                .models
                .iter()
                .map(|model| model.variants.len())
                .sum::<usize>(),
            90
        );
        assert_eq!(catalog.models[0].variants[0].runtime_ref, "bge-m3:567m");
        assert_eq!(catalog.models[0].provider.as_deref(), Some("BAAI"));
        assert_eq!(catalog.models[0].variants[0].download_item_count, Some(3));
        assert_eq!(
            catalog.models[0].variants[0].context_window_tokens,
            Some(8_192)
        );
        assert_eq!(
            catalog.models[0].variants[0]
                .artifact
                .as_ref()
                .and_then(|artifact| artifact.size_bytes),
            Some(1_157_672_605)
        );
        assert_eq!(catalog.models[0].license.profile_id.as_deref(), Some("mit"));
        assert_eq!(catalog.models[0].license.commercial_use, "permitted");
        let Some(evaluated) = catalog
            .models
            .iter()
            .flat_map(|model| &model.variants)
            .find(|variant| variant.runtime_ref == "deepseek-r1:14b")
        else {
            panic!("the evaluated DeepSeek variant should be present");
        };
        assert_eq!(evaluated.external_evaluations.len(), 1);
        assert_eq!(evaluated.external_evaluations[0].publisher, "Onyx");
        assert_eq!(
            evaluated.external_evaluations[0].overall_tier,
            OverallTier::D
        );
        assert_eq!(
            evaluated.external_evaluations[0].source.url,
            "https://onyx.app/self-hosted-llm-leaderboard"
        );
        let ollama = &catalog.runtimes[0];
        assert_eq!(ollama.install.strategy.0, "archive");
        assert_eq!(ollama.install.version, "0.32.1");
        assert_eq!(
            ollama
                .install
                .urls_by_platform
                .get(&Platform::WindowsX86_64)
                .map(String::as_str),
            Some(
                "https://github.com/ollama/ollama/releases/download/v0.32.1/ollama-windows-amd64.zip"
            )
        );
        assert_eq!(
            ollama
                .install
                .sha256_by_platform
                .get(&Platform::WindowsX86_64)
                .map(String::as_str),
            Some("d5abdc21b64ee928d3c92880ac22da5e5b0a46b8b07179791dd8c711b35f8397")
        );
    }

    #[test]
    fn includes_a_dummy_catalog_model_for_ui_testing() {
        let catalog = Catalog::from_slice(VALID).unwrap();

        let Some(dummy_model) = catalog
            .models
            .iter()
            .find(|model| model.id == "dummy-test-model")
        else {
            panic!("dummy model should be present in the catalog fixture");
        };
        let Some(runtime) = catalog
            .runtimes
            .iter()
            .find(|runtime| runtime.id == dummy_model.variants[0].runtime)
        else {
            panic!("dummy model runtime should be present in the catalog fixture");
        };

        assert_eq!(dummy_model.display_name, "Dummy Test Model");
        assert_eq!(runtime.install.version, "0.0.0-dummy");
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let error = Catalog::from_slice(INVALID_SCHEMA).unwrap_err();

        assert!(matches!(
            error,
            CatalogError::UnsupportedSchemaVersion { found, .. } if found == "2"
        ));
    }

    #[test]
    fn rejects_invalid_catalog_shape() {
        let error = Catalog::from_slice(INVALID_SHAPE).unwrap_err();

        assert!(matches!(error, CatalogError::Json(_)));
    }

    #[test]
    fn round_trips_catalog() {
        let catalog = Catalog::from_slice(VALID).unwrap();
        let bytes = serde_json::to_vec(&catalog).unwrap();
        let reparsed = Catalog::from_slice(&bytes).unwrap();

        assert_eq!(catalog, reparsed);
    }
}
