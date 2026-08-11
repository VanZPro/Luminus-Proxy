use std::fmt;

pub struct LegacyCiphertext(String);

impl LegacyCiphertext {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub(crate) fn encoded(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LegacyCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LegacyCiphertext([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts() {
        assert!(!format!("{:?}", LegacyCiphertext::new("secret")).contains("secret"));
    }
}
