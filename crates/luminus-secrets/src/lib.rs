mod error;
mod resolver;
mod secret;

pub use error::SecretError;
pub use resolver::{CredentialRequest, CredentialResolver};
pub use secret::SecretString;

pub type CredentialResolverFuture<'a, C> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<C, SecretError>> + Send + 'a>>;

#[cfg(test)]
mod tests {
    use super::*;
    use luminus_core::model::{AccountId, ProviderId};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Debug, PartialEq, Eq)]
    struct SyntheticCredentials {
        access_token: SecretString,
    }

    struct SyntheticResolver {
        values: HashMap<AccountId, (ProviderId, String)>,
    }

    impl CredentialResolver<SyntheticCredentials> for SyntheticResolver {
        fn resolve<'a>(
            &'a self,
            request: &'a CredentialRequest,
        ) -> CredentialResolverFuture<'a, SyntheticCredentials> {
            Box::pin(async move {
                let Some((provider, value)) = self.values.get(&request.account_id) else {
                    return Err(SecretError::NotFound);
                };
                if provider != &request.provider_id {
                    return Err(SecretError::InvalidMaterial);
                }
                Ok(SyntheticCredentials {
                    access_token: SecretString::new(value.clone()),
                })
            })
        }
    }

    #[tokio::test]
    async fn secret_string_requires_explicit_access_and_redacts_debug() {
        let secret = SecretString::new("synthetic-secret");
        assert_eq!(secret.expose_secret(), "synthetic-secret");
        let debug = format!("{secret:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("synthetic-secret"));
    }

    #[tokio::test]
    async fn typed_resolver_is_isolated_and_object_safe() {
        let mut values = HashMap::new();
        values.insert(
            AccountId::from("a"),
            (ProviderId::from("synthetic"), "secret-a".into()),
        );
        values.insert(
            AccountId::from("b"),
            (ProviderId::from("synthetic"), "secret-b".into()),
        );
        let resolver: Arc<dyn CredentialResolver<SyntheticCredentials>> =
            Arc::new(SyntheticResolver { values });
        let a = resolver
            .resolve(&CredentialRequest::new("a", "synthetic"))
            .await
            .unwrap();
        let b = resolver
            .resolve(&CredentialRequest::new("b", "synthetic"))
            .await
            .unwrap();
        assert_eq!(a.access_token.expose_secret(), "secret-a");
        assert_eq!(b.access_token.expose_secret(), "secret-b");
        assert_eq!(
            resolver
                .resolve(&CredentialRequest::new("missing", "synthetic"))
                .await,
            Err(SecretError::NotFound)
        );
        assert_eq!(
            resolver
                .resolve(&CredentialRequest::new("a", "other"))
                .await,
            Err(SecretError::InvalidMaterial)
        );
    }

    #[test]
    fn secret_errors_do_not_contain_secret_material() {
        for error in [
            SecretError::NotFound,
            SecretError::InvalidMaterial,
            SecretError::DecryptionFailed,
            SecretError::Unavailable,
            SecretError::Internal,
        ] {
            let text = error.to_string();
            assert!(!text.contains("secret"));
            assert!(!text.contains("token"));
        }
    }
}
