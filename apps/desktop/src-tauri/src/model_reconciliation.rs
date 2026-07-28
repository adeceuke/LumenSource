use lumen_source_catalog::Catalog;
use lumen_source_runtime::InstalledModel;

use crate::bridge_types::{local_target_id, PersistedModelEntry};

pub(crate) const DUMMY_RUNTIME: &str = "dummy-runtime";

pub(crate) fn reconcile_unavailable_models(
    catalog: &Catalog,
    mut persisted: Vec<PersistedModelEntry>,
    dummy_installed: &[InstalledModel],
    dummy_running: &[String],
) -> Vec<PersistedModelEntry> {
    for entry in &mut persisted {
        let dummy_reference = catalog.models.iter().find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| variant.id == entry.model_id && variant.runtime == DUMMY_RUNTIME)
                .map(|variant| variant.runtime_ref.as_str())
        });
        entry.running = dummy_reference
            .is_some_and(|reference| dummy_running.iter().any(|running| running == reference));
    }
    upsert_dummy_models(catalog, &mut persisted, dummy_installed, dummy_running);
    sort_models(&mut persisted);
    persisted
}

pub(crate) fn reconcile_models(
    catalog: Catalog,
    mut persisted: Vec<PersistedModelEntry>,
    installed: Vec<InstalledModel>,
    dummy_installed: &[InstalledModel],
    running: &[String],
) -> Vec<PersistedModelEntry> {
    let mut result = Vec::with_capacity(installed.len());
    for installed_model in installed {
        let catalog_match = catalog.models.iter().find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| {
                    variant.runtime == "ollama"
                        && same_ollama_reference(&variant.runtime_ref, &installed_model.name)
                })
                .map(|variant| (model, variant))
        });
        let mut previous_entries = Vec::new();
        let mut remaining = Vec::with_capacity(persisted.len());
        for entry in persisted {
            let matches_installed = entry
                .runtime_model_id
                .as_deref()
                .is_some_and(|name| same_ollama_reference(name, &installed_model.name))
                || catalog_match.is_some_and(|(_, variant)| entry.model_id == variant.id);
            if matches_installed {
                previous_entries.push(entry);
            } else {
                remaining.push(entry);
            }
        }
        persisted = remaining;
        let previous_entries = if previous_entries.is_empty() {
            vec![None]
        } else {
            previous_entries.into_iter().map(Some).collect()
        };
        let is_running = running
            .iter()
            .any(|name| same_ollama_reference(name, &installed_model.name));

        for previous in previous_entries {
            let entry = if let Some((model, variant)) = catalog_match {
                let runtime_version = catalog
                    .runtimes
                    .iter()
                    .find(|runtime| runtime.id == variant.runtime)
                    .map(|runtime| runtime.install.version.clone())
                    .unwrap_or_else(|| "unknown".to_owned());
                PersistedModelEntry {
                    id: previous
                        .as_ref()
                        .map(|entry| entry.id.clone())
                        .unwrap_or_else(|| discovered_id(&installed_model)),
                    name: previous
                        .as_ref()
                        .map(|entry| entry.name.clone())
                        .unwrap_or_else(|| model.display_name.clone()),
                    model_id: variant.id.clone(),
                    model_name: model.display_name.clone(),
                    runtime_model_id: Some(installed_model.name.clone()),
                    version: runtime_version,
                    location: "local".to_owned(),
                    target_id: local_target_id(),
                    target_name: None,
                    running: is_running,
                    managed: true,
                    digest: installed_model.digest.clone(),
                    size_bytes: installed_model.size_bytes,
                    license_basis: previous
                        .as_ref()
                        .and_then(|entry| entry.license_basis.clone()),
                    license_reference: previous
                        .as_ref()
                        .and_then(|entry| entry.license_reference.clone()),
                    license_acknowledged_at: previous
                        .as_ref()
                        .and_then(|entry| entry.license_acknowledged_at.clone()),
                    license_profile_id: previous
                        .as_ref()
                        .and_then(|entry| entry.license_profile_id.clone()),
                    license_name: previous
                        .as_ref()
                        .and_then(|entry| entry.license_name.clone()),
                    license_url: previous
                        .as_ref()
                        .and_then(|entry| entry.license_url.clone()),
                    license_reviewed_at: previous
                        .as_ref()
                        .and_then(|entry| entry.license_reviewed_at.clone()),
                    license_catalog_version: previous
                        .as_ref()
                        .and_then(|entry| entry.license_catalog_version.clone()),
                    logs: previous.as_ref().map_or_else(
                        || vec!["Discovered in the local Ollama model store.".to_owned()],
                        |entry| entry.logs.clone(),
                    ),
                }
            } else {
                PersistedModelEntry {
                    id: previous
                        .as_ref()
                        .map(|entry| entry.id.clone())
                        .unwrap_or_else(|| discovered_id(&installed_model)),
                    name: previous
                        .as_ref()
                        .map(|entry| entry.name.clone())
                        .unwrap_or_else(|| installed_model.name.clone()),
                    model_id: format!("external:{}", installed_model.name),
                    model_name: installed_model.name.clone(),
                    runtime_model_id: Some(installed_model.name.clone()),
                    version: "External Ollama model".to_owned(),
                    location: "local".to_owned(),
                    target_id: local_target_id(),
                    target_name: None,
                    running: is_running,
                    managed: false,
                    digest: installed_model.digest.clone(),
                    size_bytes: installed_model.size_bytes,
                    license_basis: previous
                        .as_ref()
                        .and_then(|entry| entry.license_basis.clone()),
                    license_reference: previous
                        .as_ref()
                        .and_then(|entry| entry.license_reference.clone()),
                    license_acknowledged_at: previous
                        .as_ref()
                        .and_then(|entry| entry.license_acknowledged_at.clone()),
                    license_profile_id: previous
                        .as_ref()
                        .and_then(|entry| entry.license_profile_id.clone()),
                    license_name: previous
                        .as_ref()
                        .and_then(|entry| entry.license_name.clone()),
                    license_url: previous
                        .as_ref()
                        .and_then(|entry| entry.license_url.clone()),
                    license_reviewed_at: previous
                        .as_ref()
                        .and_then(|entry| entry.license_reviewed_at.clone()),
                    license_catalog_version: previous
                        .as_ref()
                        .and_then(|entry| entry.license_catalog_version.clone()),
                    logs: previous.as_ref().map_or_else(
                        || vec!["Discovered outside the active Lumen Source catalog.".to_owned()],
                        |entry| entry.logs.clone(),
                    ),
                }
            };
            result.push(entry);
        }
    }
    for mut entry in persisted {
        let dummy_variant = catalog.models.iter().find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| variant.id == entry.model_id && variant.runtime == DUMMY_RUNTIME)
        });
        if let Some(variant) = dummy_variant {
            entry.running = running.iter().any(|model| model == &variant.runtime_ref);
            result.push(entry);
        }
    }
    upsert_dummy_models(&catalog, &mut result, dummy_installed, running);
    sort_models(&mut result);
    result
}

fn upsert_dummy_models(
    catalog: &Catalog,
    models: &mut Vec<PersistedModelEntry>,
    installed: &[InstalledModel],
    running: &[String],
) {
    for installed_model in installed {
        let Some((model, variant)) = catalog.models.iter().find_map(|model| {
            model
                .variants
                .iter()
                .find(|variant| {
                    variant.runtime == DUMMY_RUNTIME && variant.runtime_ref == installed_model.name
                })
                .map(|variant| (model, variant))
        }) else {
            continue;
        };
        let is_running = running.iter().any(|name| name == &installed_model.name);
        let mut found = false;
        for entry in models.iter_mut().filter(|entry| {
            entry.model_id == variant.id
                || entry.runtime_model_id.as_deref() == Some(installed_model.name.as_str())
        }) {
            found = true;
            entry.running = is_running;
            entry.digest = installed_model.digest.clone();
            entry.size_bytes = installed_model.size_bytes;
        }
        if found {
            continue;
        }
        let version = catalog
            .runtimes
            .iter()
            .find(|runtime| runtime.id == variant.runtime)
            .map(|runtime| runtime.install.version.clone())
            .unwrap_or_else(|| "unknown".to_owned());
        models.push(PersistedModelEntry {
            id: format!("dummy:{}", installed_model.name),
            name: model.display_name.clone(),
            model_id: variant.id.clone(),
            model_name: model.display_name.clone(),
            runtime_model_id: Some(installed_model.name.clone()),
            version,
            location: "local".to_owned(),
            target_id: local_target_id(),
            target_name: None,
            running: is_running,
            managed: true,
            digest: installed_model.digest.clone(),
            size_bytes: installed_model.size_bytes,
            license_basis: None,
            license_reference: None,
            license_acknowledged_at: None,
            license_profile_id: None,
            license_name: None,
            license_url: None,
            license_reviewed_at: None,
            license_catalog_version: None,
            logs: vec!["Discovered in the dummy test runtime.".to_owned()],
        });
    }
}

pub(crate) fn with_remote_models(
    mut local_models: Vec<PersistedModelEntry>,
    remote_models: Vec<PersistedModelEntry>,
) -> Vec<PersistedModelEntry> {
    local_models.extend(remote_models);
    sort_models(&mut local_models);
    local_models
}

fn sort_models(models: &mut [PersistedModelEntry]) {
    models.sort_by(|left, right| {
        right
            .running
            .cmp(&left.running)
            .then_with(|| left.name.cmp(&right.name))
    });
}

pub(crate) fn same_ollama_reference(left: &str, right: &str) -> bool {
    normalize_ollama_reference(left) == normalize_ollama_reference(right)
}

fn normalize_ollama_reference(reference: &str) -> String {
    let reference = reference.trim();
    let last_slash = reference.rfind('/').map_or(0, |index| index + 1);
    if reference[last_slash..].contains(':') {
        reference.to_owned()
    } else {
        format!("{reference}:latest")
    }
}

fn discovered_id(model: &InstalledModel) -> String {
    format!(
        "ollama:{}",
        model.digest.as_deref().unwrap_or(model.name.as_str())
    )
}
