use luminus_core::{
    model::{ModelInfo, ProviderId},
    provider::ProviderAdapter,
};
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn ProviderAdapter>>,
}
impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, provider: Arc<dyn ProviderAdapter>) {
        self.providers.push(provider);
    }
    pub fn get(&self, id: &ProviderId) -> Option<Arc<dyn ProviderAdapter>> {
        self.providers
            .iter()
            .find(|p| p.provider_id() == id)
            .cloned()
    }
    pub fn list(&self) -> Vec<ProviderId> {
        self.providers
            .iter()
            .map(|p| p.provider_id().clone())
            .collect()
    }
    pub fn models(&self) -> Vec<ModelInfo> {
        self.providers.iter().flat_map(|p| p.models()).collect()
    }
}
