//! Compatibility filtering, ranking, and right-sizing recommendations.

use lumen_source_catalog::{
    Accelerator, Catalog, ModelEntry, ModelVariant, OperatingSystem, Requirements,
};
use lumen_source_hardware::{AcceleratorKind, HardwareFacts};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecommendationRequest {
    /// Primary catalog use case, for example `coding` or `general-assistant`.
    pub use_case: Option<String>,
    /// Additional desired tags matched against a variant's `recommended_for`.
    #[serde(default)]
    pub priorities: Vec<String>,
    /// Zero means no limit.
    #[serde(default)]
    pub max_results: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecommendationReport {
    pub recommendations: Vec<Recommendation>,
    pub exclusions: Vec<Exclusion>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recommendation {
    pub model_id: String,
    pub model_name: String,
    pub variant_id: String,
    pub runtime_id: String,
    pub runtime_ref: String,
    pub score: f64,
    pub explanations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusion {
    pub model_id: String,
    pub variant_id: String,
    pub reasons: Vec<String>,
}

/// Filters every model variant for hard compatibility, then ranks compatible variants.
pub fn recommend(
    catalog: &Catalog,
    hardware: &HardwareFacts,
    request: &RecommendationRequest,
) -> RecommendationReport {
    let platform = host_platform(hardware);
    let accelerators = host_accelerators(hardware);
    let mut recommendations = Vec::new();
    let mut exclusions = Vec::new();

    for model in &catalog.models {
        for variant in &model.variants {
            let reasons =
                exclusion_reasons(&variant.requirements, hardware, platform, &accelerators);
            if reasons.is_empty() {
                recommendations.push(score_variant(
                    model,
                    variant,
                    hardware,
                    request,
                    &accelerators,
                ));
            } else {
                exclusions.push(Exclusion {
                    model_id: model.id.clone(),
                    variant_id: variant.id.clone(),
                    reasons,
                });
            }
        }
    }

    recommendations.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.model_id.cmp(&right.model_id))
            .then_with(|| left.variant_id.cmp(&right.variant_id))
    });
    if request.max_results > 0 {
        recommendations.truncate(request.max_results);
    }
    RecommendationReport {
        recommendations,
        exclusions,
    }
}

fn exclusion_reasons(
    requirements: &Requirements,
    hardware: &HardwareFacts,
    platform: Option<OperatingSystem>,
    accelerators: &BTreeSet<Accelerator>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(supported) = &requirements.os {
        match platform {
            Some(host) if supported.contains(&host) => {}
            Some(host) => reasons.push(format!(
                "operating system {host:?} is unsupported; requires one of {supported:?}"
            )),
            None => reasons.push(format!(
                "host platform is not recognized; requires one of {supported:?}"
            )),
        }
    }

    if !requirements.accelerators.is_empty()
        && !requirements
            .accelerators
            .iter()
            .any(|required| accelerators.contains(required))
    {
        reasons.push(format!(
            "accelerator is incompatible; requires one of {:?}, detected {:?}",
            requirements.accelerators, accelerators
        ));
    }

    let total_ram_gb = bytes_to_gib(hardware.total_ram_bytes);
    if total_ram_gb < requirements.min_ram_gb {
        reasons.push(format!(
            "requires {:.1} GiB RAM, but only {:.1} GiB is installed",
            requirements.min_ram_gb, total_ram_gb
        ));
    }

    let storage_gb = bytes_to_gib(hardware.storage.available_bytes);
    if storage_gb < requirements.min_storage_gb {
        reasons.push(format!(
            "requires {:.1} GiB free storage, but only {:.1} GiB is available",
            requirements.min_storage_gb, storage_gb
        ));
    }

    if let Some(required_vram) = requirements.min_vram_gb {
        let available_vram = compatible_vram_gb(hardware, &requirements.accelerators);
        match available_vram {
            Some(vram) if vram >= required_vram => {}
            Some(vram) => reasons.push(format!(
                "requires {:.1} GiB VRAM, but the best compatible GPU has {:.1} GiB",
                required_vram, vram
            )),
            None => reasons.push(format!(
                "requires {:.1} GiB VRAM, but no compatible GPU reports VRAM capacity",
                required_vram
            )),
        }
    }
    reasons
}

fn score_variant(
    model: &ModelEntry,
    variant: &ModelVariant,
    hardware: &HardwareFacts,
    request: &RecommendationRequest,
    host_accelerators: &BTreeSet<Accelerator>,
) -> Recommendation {
    let mut score = 0.0;
    let mut explanations = Vec::new();

    if let Some(use_case) = request.use_case.as_deref() {
        if contains_tag(&model.use_cases, use_case) {
            score += 35.0;
            explanations.push(format!("model is designed for the {use_case} use case"));
        } else {
            explanations.push(format!(
                "model is compatible, but does not explicitly list {use_case}"
            ));
        }
        if contains_tag(&variant.recommended_for, use_case) {
            score += 15.0;
            explanations.push(format!("this variant is recommended for {use_case}"));
        }
    }

    let priority_matches: Vec<&str> = request
        .priorities
        .iter()
        .filter(|priority| contains_tag(&variant.recommended_for, priority))
        .map(String::as_str)
        .collect();
    if !priority_matches.is_empty() {
        score += 10.0 * priority_matches.len() as f64;
        explanations.push(format!(
            "matches preferred profile: {}",
            priority_matches.join(", ")
        ));
    }

    let gpu_match = variant.requirements.accelerators.iter().any(|accelerator| {
        *accelerator != Accelerator::Cpu && host_accelerators.contains(accelerator)
    });
    if gpu_match {
        score += 20.0;
        explanations.push("uses a detected GPU accelerator".to_owned());
    } else if variant
        .requirements
        .accelerators
        .contains(&Accelerator::Cpu)
    {
        score += 5.0;
        explanations.push("can run on the detected CPU".to_owned());
    }

    let ram_headroom = headroom_ratio(
        bytes_to_gib(hardware.available_ram_bytes),
        variant.requirements.min_ram_gb,
    );
    score += 15.0 * ram_headroom;
    explanations.push(format!(
        "{:.1} GiB RAM remains available against a {:.1} GiB minimum",
        bytes_to_gib(hardware.available_ram_bytes),
        variant.requirements.min_ram_gb
    ));

    let storage_headroom = headroom_ratio(
        bytes_to_gib(hardware.storage.available_bytes),
        variant.requirements.min_storage_gb,
    );
    score += 5.0 * storage_headroom;
    explanations.push(format!(
        "{:.1} GiB free storage is available",
        bytes_to_gib(hardware.storage.available_bytes)
    ));

    if let Some(required_vram) = variant.requirements.min_vram_gb {
        if let Some(vram) = compatible_vram_gb(hardware, &variant.requirements.accelerators) {
            score += 10.0 * headroom_ratio(vram, required_vram);
            explanations.push(format!(
                "{vram:.1} GiB compatible VRAM exceeds the {required_vram:.1} GiB minimum"
            ));
        }
    }

    Recommendation {
        model_id: model.id.clone(),
        model_name: model.display_name.clone(),
        variant_id: variant.id.clone(),
        runtime_id: variant.runtime.clone(),
        runtime_ref: variant.runtime_ref.clone(),
        score: round_score(score),
        explanations,
    }
}

fn contains_tag(tags: &[String], needle: &str) -> bool {
    tags.iter().any(|tag| tag.eq_ignore_ascii_case(needle))
}

fn host_platform(hardware: &HardwareFacts) -> Option<OperatingSystem> {
    match hardware.os.family.to_ascii_lowercase().as_str() {
        "linux" => Some(OperatingSystem::Linux),
        "macos" | "darwin" => Some(OperatingSystem::Darwin),
        "windows" => Some(OperatingSystem::Windows),
        _ => None,
    }
}

fn host_accelerators(hardware: &HardwareFacts) -> BTreeSet<Accelerator> {
    let mut result = BTreeSet::from([Accelerator::Cpu]);
    for accelerator in &hardware.accelerators {
        match accelerator.kind {
            AcceleratorKind::Nvidia => {
                result.insert(Accelerator::Cuda);
            }
            AcceleratorKind::Amd => {
                result.insert(Accelerator::Rocm);
            }
            AcceleratorKind::Intel | AcceleratorKind::Other => {}
        }
    }
    result
}

fn compatible_vram_gb(
    hardware: &HardwareFacts,
    required_accelerators: &[Accelerator],
) -> Option<f64> {
    hardware
        .accelerators
        .iter()
        .filter(|device| match device.kind {
            AcceleratorKind::Nvidia => required_accelerators.contains(&Accelerator::Cuda),
            AcceleratorKind::Amd => required_accelerators.contains(&Accelerator::Rocm),
            AcceleratorKind::Intel | AcceleratorKind::Other => false,
        })
        .filter_map(|device| device.total_vram_bytes)
        .max()
        .map(bytes_to_gib)
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / GIB
}

/// Returns 0 at minimum capacity and 1 at twice the minimum.
fn headroom_ratio(available: f64, required: f64) -> f64 {
    if required <= f64::EPSILON {
        1.0
    } else {
        ((available - required) / required).clamp(0.0, 1.0)
    }
}

fn round_score(score: f64) -> f64 {
    (score * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_source_catalog::{License, ModelVariant};
    use lumen_source_hardware::{AcceleratorFacts, CpuFacts, OsFacts, StorageFacts};
    use std::path::PathBuf;

    fn gib(value: u64) -> u64 {
        value * 1024 * 1024 * 1024
    }

    fn golden_hardware() -> HardwareFacts {
        HardwareFacts {
            os: OsFacts {
                family: "linux".to_owned(),
                distribution: Some("ubuntu".to_owned()),
                version: Some("24.04".to_owned()),
                architecture: "x86_64".to_owned(),
            },
            cpu: CpuFacts {
                model: Some("Golden CPU".to_owned()),
                architecture: "x86_64".to_owned(),
                logical_cores: 16,
                physical_cores: Some(8),
            },
            total_ram_bytes: gib(32),
            available_ram_bytes: gib(24),
            storage: StorageFacts {
                mount_point: PathBuf::from("/"),
                total_bytes: gib(500),
                available_bytes: gib(200),
            },
            accelerators: vec![AcceleratorFacts {
                kind: AcceleratorKind::Nvidia,
                name: "Golden RTX".to_owned(),
                total_vram_bytes: Some(gib(12)),
                driver_version: Some("golden".to_owned()),
            }],
        }
    }

    fn variant(
        id: &str,
        min_ram_gb: f64,
        min_vram_gb: Option<f64>,
        min_storage_gb: f64,
        accelerators: Vec<Accelerator>,
        recommended_for: &[&str],
    ) -> ModelVariant {
        ModelVariant {
            id: id.to_owned(),
            runtime: "ollama".to_owned(),
            runtime_ref: format!("golden:{id}"),
            parameters_b: 7.0,
            quantization: Some("q4".to_owned()),
            requirements: Requirements {
                min_ram_gb,
                min_vram_gb,
                min_storage_gb,
                os: Some(vec![OperatingSystem::Linux]),
                accelerators,
            },
            artifact: None,
            performance_hint: None,
            recommended_for: recommended_for
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        }
    }

    fn golden_catalog() -> Catalog {
        Catalog {
            schema_version: "1".to_owned(),
            catalog_version: "golden".to_owned(),
            published_at: "2026-07-20T00:00:00Z".to_owned(),
            runtimes: Vec::new(),
            models: vec![
                ModelEntry {
                    id: "coder".to_owned(),
                    display_name: "Golden Coder".to_owned(),
                    description: "Coding model".to_owned(),
                    capabilities: vec!["text".to_owned()],
                    languages: vec!["en".to_owned()],
                    license: License {
                        spdx: Some("Apache-2.0".to_owned()),
                        name: "Apache 2.0".to_owned(),
                        url: None,
                    },
                    use_cases: vec!["coding".to_owned()],
                    variants: vec![
                        variant(
                            "gpu",
                            16.0,
                            Some(8.0),
                            10.0,
                            vec![Accelerator::Cuda],
                            &["coding", "quality"],
                        ),
                        variant(
                            "too-large",
                            64.0,
                            Some(24.0),
                            300.0,
                            vec![Accelerator::Cuda],
                            &["coding"],
                        ),
                    ],
                },
                ModelEntry {
                    id: "generic".to_owned(),
                    display_name: "Generic".to_owned(),
                    description: "General model".to_owned(),
                    capabilities: vec!["text".to_owned()],
                    languages: vec!["en".to_owned()],
                    license: License {
                        spdx: None,
                        name: "Test".to_owned(),
                        url: None,
                    },
                    use_cases: vec!["chat".to_owned()],
                    variants: vec![variant(
                        "cpu",
                        8.0,
                        None,
                        5.0,
                        vec![Accelerator::Cpu],
                        &["portable"],
                    )],
                },
            ],
        }
    }

    #[test]
    fn golden_profile_filters_and_ranks_gpu_coder_first() {
        let report = recommend(
            &golden_catalog(),
            &golden_hardware(),
            &RecommendationRequest {
                use_case: Some("coding".to_owned()),
                priorities: vec!["quality".to_owned()],
                max_results: 0,
            },
        );

        assert_eq!(report.recommendations.len(), 2);
        assert_eq!(report.recommendations[0].variant_id, "gpu");
        assert!(report.recommendations[0].score > report.recommendations[1].score);
        assert_eq!(report.exclusions.len(), 1);
        assert_eq!(report.exclusions[0].variant_id, "too-large");
        assert!(report.exclusions[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("RAM")));
    }

    #[test]
    fn golden_cpu_only_profile_gracefully_excludes_gpu_variant() {
        let mut hardware = golden_hardware();
        hardware.accelerators.clear();
        let report = recommend(
            &golden_catalog(),
            &hardware,
            &RecommendationRequest::default(),
        );

        assert_eq!(report.recommendations.len(), 1);
        assert_eq!(report.recommendations[0].variant_id, "cpu");
        assert!(report.exclusions.iter().any(|item| item.variant_id == "gpu"
            && item
                .reasons
                .iter()
                .any(|reason| reason.contains("accelerator"))));
    }

    #[test]
    fn max_results_is_deterministic() {
        let report = recommend(
            &golden_catalog(),
            &golden_hardware(),
            &RecommendationRequest {
                max_results: 1,
                ..RecommendationRequest::default()
            },
        );
        assert_eq!(report.recommendations.len(), 1);
    }
}
