use crate::{LegacyCiphertext, LegacyCredentialError};
use base64::{Engine, engine::general_purpose::STANDARD};
use luminus_secrets::SecretString;

pub fn decode(
    ciphertext: &LegacyCiphertext,
    key: &SecretString,
) -> Result<SecretString, LegacyCredentialError> {
    let key = key.expose_secret().as_bytes();
    if key.is_empty() {
        return Err(LegacyCredentialError::InvalidKey);
    }
    let data = STANDARD
        .decode(ciphertext.encoded())
        .map_err(|_| LegacyCredentialError::InvalidCiphertext)?;
    let plain: Vec<u8> = data
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect();
    String::from_utf8(plain)
        .map(SecretString::new)
        .map_err(|_| LegacyCredentialError::InvalidMaterial)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vectors_and_errors() {
        let c = LegacyCiphertext::new("AAAAAAAAAAAAABsECgAOAQYMSBU=");
        assert_eq!(
            decode(&c, &SecretString::new("synthetic-key"))
                .unwrap()
                .expose_secret(),
            "synthetic-password-a"
        );
        assert_eq!(
            decode(&LegacyCiphertext::new("!"), &SecretString::new("k")),
            Err(LegacyCredentialError::InvalidCiphertext)
        );
        assert_eq!(
            decode(&LegacyCiphertext::new("AA=="), &SecretString::new("")),
            Err(LegacyCredentialError::InvalidKey)
        );
    }
}

// No legacy encoder is exposed: XOR/Base64 is read compatibility only.
