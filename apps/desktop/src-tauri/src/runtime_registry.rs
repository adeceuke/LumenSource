use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const OLLAMA_RUNTIME: &str = "ollama";
pub const VLLM_RUNTIME: &str = "vllm";
pub const DUMMY_RUNTIME: &str = "dummy-runtime";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeId {
    #[default]
    Ollama,
    Vllm,
    #[serde(rename = "dummy-runtime")]
    Dummy,
}

impl RuntimeId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => OLLAMA_RUNTIME,
            Self::Vllm => VLLM_RUNTIME,
            Self::Dummy => DUMMY_RUNTIME,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            OLLAMA_RUNTIME => Some(Self::Ollama),
            VLLM_RUNTIME => Some(Self::Vllm),
            DUMMY_RUNTIME => Some(Self::Dummy),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeLifecycle {
    Managed,
    External,
    Simulated,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub managed_model_storage: bool,
    pub multiple_models: bool,
    pub chat: bool,
    pub embeddings: bool,
    pub pooling: bool,
    pub model_start_stop: bool,
    pub global_configuration: bool,
    pub per_model_configuration: bool,
    pub artifact_acquisition: bool,
    pub remote_connection: bool,
    pub lifecycle: Option<RuntimeLifecycle>,
}

#[derive(Clone, Debug)]
pub struct RuntimeDescriptor {
    pub id: RuntimeId,
    pub capabilities: RuntimeCapabilities,
}

#[derive(Clone, Debug)]
pub struct RuntimeRegistry {
    runtimes: BTreeMap<RuntimeId, RuntimeDescriptor>,
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        let entries = [
            RuntimeDescriptor {
                id: RuntimeId::Ollama,
                capabilities: RuntimeCapabilities {
                    managed_model_storage: true,
                    multiple_models: true,
                    chat: true,
                    embeddings: true,
                    pooling: false,
                    model_start_stop: true,
                    global_configuration: true,
                    per_model_configuration: true,
                    artifact_acquisition: true,
                    remote_connection: true,
                    lifecycle: Some(RuntimeLifecycle::Managed),
                },
            },
            RuntimeDescriptor {
                id: RuntimeId::Vllm,
                capabilities: RuntimeCapabilities {
                    managed_model_storage: false,
                    multiple_models: false,
                    chat: true,
                    embeddings: true,
                    pooling: true,
                    model_start_stop: false,
                    global_configuration: false,
                    per_model_configuration: true,
                    artifact_acquisition: false,
                    remote_connection: false,
                    lifecycle: Some(RuntimeLifecycle::External),
                },
            },
            RuntimeDescriptor {
                id: RuntimeId::Dummy,
                capabilities: RuntimeCapabilities {
                    managed_model_storage: true,
                    multiple_models: true,
                    chat: false,
                    embeddings: false,
                    pooling: false,
                    model_start_stop: true,
                    global_configuration: false,
                    per_model_configuration: false,
                    artifact_acquisition: false,
                    remote_connection: false,
                    lifecycle: Some(RuntimeLifecycle::Simulated),
                },
            },
        ];
        Self {
            runtimes: entries
                .into_iter()
                .map(|descriptor| (descriptor.id, descriptor))
                .collect(),
        }
    }
}

impl RuntimeRegistry {
    pub fn resolve(&self, id: RuntimeId) -> &RuntimeDescriptor {
        self.runtimes
            .get(&id)
            .unwrap_or_else(|| unreachable!("all RuntimeId values are registered"))
    }

    pub fn resolve_name(&self, id: &str) -> Option<&RuntimeDescriptor> {
        RuntimeId::parse(id).map(|id| self.resolve(id))
    }

    pub fn supports(&self, id: &str) -> bool {
        self.resolve_name(id).is_some()
    }
}

pub fn capabilities_for(runtime_id: &str) -> RuntimeCapabilities {
    RuntimeRegistry::default()
        .resolve_name(runtime_id)
        .map(|runtime| runtime.capabilities.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_runtime_id_has_a_descriptor() {
        let registry = RuntimeRegistry::default();
        for id in [RuntimeId::Ollama, RuntimeId::Vllm, RuntimeId::Dummy] {
            assert_eq!(registry.resolve(id).id, id);
        }
    }

    #[test]
    fn external_vllm_has_inference_but_no_lifecycle_control() {
        let registry = RuntimeRegistry::default();
        let capabilities = &registry.resolve(RuntimeId::Vllm).capabilities;
        assert!(capabilities.chat);
        assert!(capabilities.embeddings);
        assert!(!capabilities.model_start_stop);
        assert_eq!(capabilities.lifecycle, Some(RuntimeLifecycle::External));
    }
}
