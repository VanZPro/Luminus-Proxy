use std::{future::Future, pin::Pin};

use super::{ProviderContext, ProviderError};
use crate::{
    model::ModelInfo,
    protocol::{CanonicalRequest, CanonicalResponse},
};

pub trait ProviderAdapter: Send + Sync {
    fn provider_id(&self) -> &crate::model::ProviderId;
    fn models(&self) -> Vec<ModelInfo>;
    fn execute<'a>(
        &'a self,
        request: &'a CanonicalRequest,
        context: &'a ProviderContext,
    ) -> Pin<Box<dyn Future<Output = Result<CanonicalResponse, ProviderError>> + Send + 'a>>;
}
