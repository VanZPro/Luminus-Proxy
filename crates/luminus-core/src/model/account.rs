use super::ProviderId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountId(pub String);

impl From<String> for AccountId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for AccountId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountDescriptor {
    pub id: AccountId,
    pub provider: ProviderId,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_debug_contains_no_credentials() {
        let descriptor = AccountDescriptor {
            id: AccountId::from("account-1"),
            provider: ProviderId::from("fake"),
            enabled: true,
        };
        let debug = format!("{descriptor:?}");
        assert!(!debug.contains("key"));
        assert!(debug.contains("account-1"));
    }
}
