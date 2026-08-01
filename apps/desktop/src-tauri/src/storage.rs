use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::bridge_types::PersistedModelEntry;
use crate::settings::ApplicationSettings;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageReport {
    pub scanned_at: String,
    pub total_bytes: u64,
    pub reclaimable_bytes: u64,
    pub entries: Vec<StorageEntry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageEntry {
    pub id: String,
    pub category: String,
    pub label: String,
    pub path: Option<String>,
    pub size_bytes: u64,
    pub exact: bool,
    pub shared: bool,
    pub owners: Vec<String>,
    pub cleanup_eligible: bool,
    pub cleanup_effect: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub removed_bytes: u64,
    pub scope: String,
    pub effect: String,
}

pub fn storage_report(
    settings: &ApplicationSettings,
    models: &[PersistedModelEntry],
) -> StorageReport {
    let mut entries = Vec::new();
    for model in models {
        entries.push(StorageEntry {
            id: format!("model:{}", model.id),
            category: "model".to_owned(),
            label: format!("{} ({})", model.name, model.runtime_id),
            path: None,
            size_bytes: model.size_bytes.unwrap_or_default(),
            exact: model.size_bytes.is_some() && model.digest.is_some(),
            shared: false,
            owners: vec![model.name.clone()],
            cleanup_eligible: false,
            cleanup_effect:
                "Remove this model from its model page. Shared runtime caches are retained."
                    .to_owned(),
        });
    }

    push_directory(
        &mut entries,
        "ollama-store",
        "ollama",
        "Ollama model storage",
        settings.storage.model_directory.as_deref(),
        true,
        models
            .iter()
            .filter(|model| model.runtime_id == "ollama")
            .map(|model| model.name.clone())
            .collect(),
        false,
        "Managed and externally discovered Ollama weights. Remove individual models from the model library.",
    );
    push_directory(
        &mut entries,
        "hugging-face-cache",
        "hugging-face",
        "Hugging Face weight cache",
        settings.vllm.hugging_face_cache_directory.as_deref(),
        true,
        models
            .iter()
            .filter(|model| model.runtime_id == "vllm")
            .map(|model| model.name.clone())
            .collect(),
        false,
        "Shared by managed vLLM services. Deleting it requires an explicit shared-cache cleanup and models must be downloaded again.",
    );
    let runtime_root =
        dirs::data_local_dir().map(|directory| directory.join("lumen-source").join("runtimes"));
    push_directory(
        &mut entries,
        "installed-runtimes",
        "runtime",
        "Installed Lumen Source runtimes",
        runtime_root.as_deref(),
        true,
        Vec::new(),
        false,
        "Runtime executables managed by Lumen Source. They are retained while models may depend on them.",
    );
    push_runtime_downloads(&mut entries, runtime_root.as_deref());
    let cache_root = settings.storage.cache_directory.as_deref();
    push_directory(
        &mut entries,
        "temporary",
        "temporary",
        "Temporary and incomplete downloads",
        cache_root.map(|path| path.join("temporary")).as_deref(),
        false,
        Vec::new(),
        true,
        "Only Lumen Source temporary files are removed. Runtime model stores are not touched.",
    );
    push_directory(
        &mut entries,
        "vllm-compile-cache",
        "vllm-cache",
        "vLLM compile cache",
        cache_root.map(|path| path.join("vllm")).as_deref(),
        true,
        models
            .iter()
            .filter(|model| model.runtime_id == "vllm")
            .map(|model| model.name.clone())
            .collect(),
        true,
        "Compiled kernels are removed and will be generated again when vLLM starts.",
    );
    let managed_vllm_owners = models
        .iter()
        .filter(|model| {
            model.runtime_id == "vllm"
                && model
                    .model_settings
                    .as_ref()
                    .and_then(|settings| settings.runtime_management_mode)
                    == Some(crate::settings::RuntimeManagementMode::Managed)
        })
        .map(|model| model.name.clone())
        .collect::<Vec<_>>();
    if !managed_vllm_owners.is_empty() {
        entries.push(StorageEntry {
            id: "managed-vllm-volumes".to_owned(),
            category: "vllm-cache".to_owned(),
            label: "Managed vLLM container volumes".to_owned(),
            path: Some(
                "container volumes: lumensource-huggingface, lumensource-vllm-cache".to_owned(),
            ),
            size_bytes: 0,
            exact: false,
            shared: true,
            owners: managed_vllm_owners,
            cleanup_eligible: true,
            cleanup_effect: "Both shared managed-vLLM volumes are removed. Containers remain, but weights must be downloaded and kernels compiled again.".to_owned(),
        });
    }

    entries.sort_by(|left, right| {
        right
            .cleanup_eligible
            .cmp(&left.cleanup_eligible)
            .then_with(|| right.size_bytes.cmp(&left.size_bytes))
            .then_with(|| left.label.cmp(&right.label))
    });
    let total_bytes = entries
        .iter()
        .filter(|entry| entry.category != "model" && entry.id != "runtime-downloads")
        .map(|entry| entry.size_bytes)
        .sum();
    let reclaimable_bytes = entries
        .iter()
        .filter(|entry| entry.cleanup_eligible)
        .map(|entry| entry.size_bytes)
        .sum();
    StorageReport {
        scanned_at: chrono::Utc::now().to_rfc3339(),
        total_bytes,
        reclaimable_bytes,
        entries,
    }
}

fn push_runtime_downloads(entries: &mut Vec<StorageEntry>, runtime_root: Option<&Path>) {
    let usage = runtime_root
        .map(runtime_download_usage)
        .unwrap_or(DirectoryUsage {
            bytes: 0,
            exact: true,
        });
    entries.push(StorageEntry {
        id: "runtime-downloads".to_owned(),
        category: "runtime".to_owned(),
        label: "Incomplete runtime downloads".to_owned(),
        path: runtime_root.map(|path| {
            format!(
                "{} (*.download and *.staging only)",
                path.display()
            )
        }),
        size_bytes: usage.bytes,
        exact: usage.exact,
        shared: false,
        owners: Vec::new(),
        cleanup_eligible: usage.bytes > 0,
        cleanup_effect:
            "Only interrupted runtime archives and staging directories are removed. Installed runtime executables are retained."
                .to_owned(),
    });
}

#[allow(clippy::too_many_arguments)]
fn push_directory(
    entries: &mut Vec<StorageEntry>,
    id: &str,
    category: &str,
    label: &str,
    path: Option<&Path>,
    shared: bool,
    owners: Vec<String>,
    cleanup_eligible: bool,
    cleanup_effect: &str,
) {
    let usage = path.map(directory_usage).unwrap_or_default();
    entries.push(StorageEntry {
        id: id.to_owned(),
        category: category.to_owned(),
        label: label.to_owned(),
        path: path.map(|path| path.display().to_string()),
        size_bytes: usage.bytes,
        exact: usage.exact,
        shared,
        owners,
        cleanup_eligible,
        cleanup_effect: cleanup_effect.to_owned(),
    });
}

#[derive(Default)]
struct DirectoryUsage {
    bytes: u64,
    exact: bool,
}

fn directory_usage(path: &Path) -> DirectoryUsage {
    if !path.exists() {
        return DirectoryUsage {
            bytes: 0,
            exact: true,
        };
    }
    let mut usage = DirectoryUsage {
        bytes: 0,
        exact: true,
    };
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(children) = fs::read_dir(&directory) else {
            usage.exact = false;
            continue;
        };
        for child in children {
            let Ok(child) = child else {
                usage.exact = false;
                continue;
            };
            let Ok(metadata) = fs::symlink_metadata(child.path()) else {
                usage.exact = false;
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(child.path());
            } else if metadata.is_file() {
                usage.bytes = usage.bytes.saturating_add(metadata.len());
            }
        }
    }
    usage
}

pub fn cleanup_directory(
    settings: &ApplicationSettings,
    entry_id: &str,
    confirmed: bool,
) -> Result<CleanupReport, String> {
    if !confirmed {
        return Err("Confirm the exact cleanup scope before removing files.".to_owned());
    }
    if entry_id == "runtime-downloads" {
        let runtime_root = dirs::data_local_dir()
            .ok_or_else(|| "No local application data directory is available.".to_owned())?
            .join("lumen-source")
            .join("runtimes");
        return cleanup_runtime_downloads_at(&runtime_root);
    }
    let cache_root = settings
        .storage
        .cache_directory
        .as_deref()
        .ok_or_else(|| "No Lumen Source cache directory is configured.".to_owned())?;
    let (relative, scope, effect) = match entry_id {
        "temporary" => (
            "temporary",
            "Lumen Source temporary and incomplete downloads",
            "Runtime model stores and completed downloads were retained.",
        ),
        "vllm-compile-cache" => (
            "vllm",
            "Lumen Source vLLM compile cache",
            "Managed servers and Hugging Face model weights were retained; kernels will be rebuilt.",
        ),
        _ => {
            return Err(
                "This storage category cannot be removed through cache cleanup.".to_owned(),
            )
        }
    };
    let target = cache_root.join(relative);
    ensure_child(cache_root, &target)?;
    let before = directory_usage(&target).bytes;
    if target.exists() {
        fs::remove_dir_all(&target)
            .map_err(|error| format!("Could not remove {}: {error}", target.display()))?;
    }
    Ok(CleanupReport {
        removed_bytes: before,
        scope: scope.to_owned(),
        effect: effect.to_owned(),
    })
}

fn cleanup_runtime_downloads_at(runtime_root: &Path) -> Result<CleanupReport, String> {
    let candidates = runtime_download_candidates(runtime_root).0;
    let removed_bytes = candidates
        .iter()
        .map(|candidate| {
            if candidate.is_dir() {
                directory_usage(candidate).bytes
            } else {
                candidate
                    .metadata()
                    .map(|metadata| metadata.len())
                    .unwrap_or(0)
            }
        })
        .sum();
    for candidate in candidates {
        ensure_child(runtime_root, &candidate)?;
        let result = if candidate.is_dir() {
            fs::remove_dir_all(&candidate)
        } else {
            fs::remove_file(&candidate)
        };
        result.map_err(|error| {
            format!(
                "Could not remove the incomplete runtime download {}: {error}. Close any process using that temporary file and retry.",
                candidate.display()
            )
        })?;
    }
    Ok(CleanupReport {
        removed_bytes,
        scope: "Incomplete Lumen Source runtime downloads".to_owned(),
        effect: "Installed runtimes and model weights were retained.".to_owned(),
    })
}

fn runtime_download_usage(runtime_root: &Path) -> DirectoryUsage {
    let (candidates, exact) = runtime_download_candidates(runtime_root);
    DirectoryUsage {
        bytes: candidates
            .iter()
            .map(|candidate| {
                if candidate.is_dir() {
                    directory_usage(candidate).bytes
                } else {
                    candidate
                        .metadata()
                        .map(|metadata| metadata.len())
                        .unwrap_or(0)
                }
            })
            .sum(),
        exact,
    }
}

fn runtime_download_candidates(runtime_root: &Path) -> (Vec<PathBuf>, bool) {
    if !runtime_root.exists() {
        return (Vec::new(), true);
    }
    let mut candidates = Vec::new();
    let mut pending = vec![runtime_root.to_path_buf()];
    let mut exact = true;
    while let Some(directory) = pending.pop() {
        let Ok(children) = fs::read_dir(directory) else {
            exact = false;
            continue;
        };
        for child in children {
            let Ok(child) = child else {
                exact = false;
                continue;
            };
            let path = child.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                exact = false;
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                if path
                    .extension()
                    .is_some_and(|extension| extension == "staging")
                {
                    candidates.push(path);
                } else {
                    pending.push(path);
                }
            } else if metadata.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "download")
            {
                candidates.push(path);
            }
        }
    }
    (candidates, exact)
}

fn ensure_child(root: &Path, target: &Path) -> Result<(), String> {
    let root = absolute_lexical(root)?;
    let target = absolute_lexical(target)?;
    if target == root || !target.starts_with(&root) {
        return Err("Refusing to clean a path outside the configured cache directory.".to_owned());
    }
    Ok(())
}

fn absolute_lexical(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| format!("Could not resolve storage path: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_rejects_model_storage_categories() {
        let settings = ApplicationSettings::default();
        assert!(cleanup_directory(&settings, "ollama-store", true).is_err());
    }

    #[test]
    fn empty_storage_report_distinguishes_estimates() {
        let report = storage_report(&ApplicationSettings::default(), &[]);
        let Some(model_store) = report
            .entries
            .iter()
            .find(|entry| entry.id == "ollama-store")
        else {
            panic!("model storage should be reported");
        };
        assert!(model_store.shared);
        assert!(!model_store.cleanup_eligible);
    }

    #[test]
    fn runtime_cleanup_retains_installed_executables() {
        let Ok(directory) = tempfile::tempdir() else {
            panic!("temporary directory should be available");
        };
        let runtime_root = directory.path().join("runtimes");
        let installed = runtime_root.join("ollama").join("1.0").join("ollama.exe");
        let incomplete = runtime_root.join("ollama").join("archive.zip.download");
        let staging = runtime_root.join("ollama").join("1.1.staging");
        assert!(fs::create_dir_all(installed.parent().unwrap_or(directory.path())).is_ok());
        assert!(fs::create_dir_all(&staging).is_ok());
        assert!(fs::write(&installed, b"installed").is_ok());
        assert!(fs::write(&incomplete, b"incomplete").is_ok());
        assert!(fs::write(staging.join("ollama.exe"), b"staged").is_ok());

        let result = cleanup_runtime_downloads_at(&runtime_root);

        assert!(result.is_ok());
        assert!(installed.exists());
        assert!(!incomplete.exists());
        assert!(!staging.exists());
    }
}
