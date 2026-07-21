use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::CatalogError;

pub const CURRENT_SCHEMA_VERSION: &str = "1";

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
        let catalog: Self = serde_json::from_slice(bytes)?;
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
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelVariant {
    pub id: String,
    pub runtime: String,
    pub runtime_ref: String,
    pub parameters_b: f64,
    pub quantization: Option<String>,
    pub requirements: Requirements,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance_hint: Option<PerformanceHint>,
    pub recommended_for: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirements {
    pub min_ram_gb: f64,
    pub min_vram_gb: Option<f64>,
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
    fn includes_a_dummy_catalog_model_for_ui_testing() {
        let catalog = Catalog::from_slice(VALID).unwrap();

        let dummy_model = catalog
            .models
            .iter()
            .find(|model| model.id == "dummy-test-model")
            .expect("dummy model should be present in the catalog fixture");
        let runtime = catalog
            .runtimes
            .iter()
            .find(|runtime| runtime.id == dummy_model.variants[0].runtime)
            .expect("dummy model runtime should be present in the catalog fixture");

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
