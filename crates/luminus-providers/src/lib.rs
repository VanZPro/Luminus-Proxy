pub mod http;
pub mod providers;

pub use providers::BlackboxConfig;
pub use providers::BlackboxProvider;

#[cfg(test)]
mod tests {
    use super::providers::{BlackboxConfig, BlackboxProvider};
    use luminus_core::provider::ProviderAdapter;

    #[test]
    fn provider_id_is_stable() {
        let provider =
            BlackboxProvider::new(BlackboxConfig::new("http://localhost", "key")).unwrap();
        assert_eq!(provider.provider_id().0, "blackbox");
    }
}
