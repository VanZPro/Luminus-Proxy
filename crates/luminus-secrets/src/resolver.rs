use std::{future::Future, pin::Pin};

use luminus_core::model::{AccountId, ProviderId};

use crate::SecretError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRequest {
    pub account_id: AccountId,
    pub provider_id: ProviderId,
}

impl CredentialRequest {
    pub fn new(account_id: impl Into<AccountId>, provider_id: impl Into<ProviderId>) -> Self {
        Self {
            account_id: account_id.into(),
            provider_id: provider_id.into(),
        }
    }
}

pub type CredentialResolverFuture<'a, C> =
    Pin<Box<dyn Future<Output = Result<C, SecretError>> + Send + 'a>>;

pub trait CredentialResolver<C>: Send + Sync {
    fn resolve<'a>(&'a self, request: &'a CredentialRequest) -> CredentialResolverFuture<'a, C>;
}
