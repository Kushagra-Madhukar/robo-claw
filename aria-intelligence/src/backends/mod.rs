use super::{CachedTool, LLMResponse, OrchestratorError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Reference to a secret that can be a literal string or a vault lookup.
#[derive(Debug, Clone)]
pub enum SecretRef {
    Literal(String),
    Vault {
        key_name: String,
        vault: aria_vault::CredentialVault,
    },
}

impl SecretRef {
    pub fn resolve(&self, domain: &str) -> Result<String, OrchestratorError> {
        match self {
            Self::Literal(s) => Ok(s.clone()),
            Self::Vault { key_name, vault } => {
                vault.retrieve_global_secret(key_name, domain).map_err(|e| {
                    OrchestratorError::LLMError(format!("Vault resolution failed: {}", e))
                })
            }
        }
    }
}

#[async_trait]
pub trait LLMBackend: Send + Sync + dyn_clone::DynClone {
    /// Query the LLM with a prompt and available tools.
    async fn query(
        &self,
        prompt: &str,
        tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError>;
}

dyn_clone::clone_trait_object!(LLMBackend);

/// Metadata for a model available from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub context_length: Option<usize>,
}

/// Trait for listing and creating LLM backends.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Unique identifier for the provider (e.g., "ollama", "openrouter").
    fn id(&self) -> &str;

    /// Human-readable name for the provider.
    fn name(&self) -> &str;

    /// List models available from this provider.
    async fn list_models(&self) -> Result<Vec<ModelMetadata>, OrchestratorError>;

    /// Create a backend instance for a specific model.
    fn create_backend(&self, model_id: &str) -> Result<Box<dyn LLMBackend>, OrchestratorError>;
}

pub mod ollama;
pub mod openrouter;

use std::collections::HashMap;
use std::sync::Arc;

/// Centralized registry of model providers.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ModelProvider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn ModelProvider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    pub fn get_provider(&self, id: &str) -> Option<Arc<dyn ModelProvider>> {
        self.providers.get(id).cloned()
    }

    pub fn providers(&self) -> Vec<Arc<dyn ModelProvider>> {
        self.providers.values().cloned().collect()
    }

    pub async fn list_all_models(&self) -> HashMap<String, Vec<ModelMetadata>> {
        let mut all = HashMap::new();
        for (id, provider) in &self.providers {
            if let Ok(models) = provider.list_models().await {
                all.insert(id.clone(), models);
            }
        }
        all
    }

    pub fn create_backend(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            OrchestratorError::LLMError(format!("Provider {} not found", provider_id))
        })?;
        provider.create_backend(model_id)
    }
}
