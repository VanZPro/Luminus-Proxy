use crate::model::{ModelId, ProviderId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContext {
    pub request_id: String,
    pub provider_id: ProviderId,
    pub model_id: ModelId,
    pub account_id: Option<String>,
}

impl ProviderContext {
    pub fn new(request_id: impl Into<String>, provider_id: ProviderId, model_id: ModelId) -> Self {
        Self {
            request_id: request_id.into(),
            provider_id,
            model_id,
            account_id: None,
        }
    }
}
