use std::fmt;

pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for SecretString {}

#[cfg(test)]
mod tests {
    use super::SecretString;

    #[test]
    fn value_is_private_and_debug_is_redacted() {
        let value = SecretString::new("synthetic");
        assert_eq!(value.expose_secret(), "synthetic");
        assert_eq!(format!("{value:?}"), "SecretString([REDACTED])");
    }
}
