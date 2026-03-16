use base64::{engine::general_purpose::STANDARD, Engine as _};

fn armor_key_data(raw: &[u8], block_type: &str) -> String {
    let encoded = STANDARD.encode(raw);
    let mut pem = String::with_capacity(encoded.len() + block_type.len() * 2 + 64);
    pem.push_str("-----BEGIN ");
    pem.push_str(block_type);
    pem.push_str("-----\n");

    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).expect("base64 output is valid ascii"));
        pem.push('\n');
    }

    pem.push_str("-----END ");
    pem.push_str(block_type);
    pem.push_str("-----\n");
    pem
}

/// Convert JACS public-key bytes into canonical PEM text for HAI APIs.
///
/// JACS may keep public keys in raw algorithm-specific byte form (for example,
/// Ed25519 and pq2025) or as PEM text (for example, RSA). This helper preserves
/// existing PEM blocks and otherwise ASCII-armors the exact key bytes without
/// lossy decoding.
#[must_use]
pub fn normalize_public_key_pem(raw: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(raw) {
        let trimmed = text.trim();
        if trimmed.contains("BEGIN PUBLIC KEY") || trimmed.contains("BEGIN RSA PUBLIC KEY") {
            let mut normalized = trimmed.replace("\r\n", "\n").replace('\r', "\n");
            if !normalized.ends_with('\n') {
                normalized.push('\n');
            }
            return normalized;
        }
    }

    armor_key_data(raw, "PUBLIC KEY")
}

#[cfg(test)]
mod tests {
    use super::normalize_public_key_pem;

    #[test]
    fn normalizes_raw_bytes_into_pem() {
        let pem = normalize_public_key_pem(&[0x34, 0x9e, 0x74, 0xd9, 0xd1, 0x60]);
        assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----\n"));
        assert!(pem.ends_with("-----END PUBLIC KEY-----\n"));
        assert!(pem.contains("NJ502dFg"));
    }

    #[test]
    fn preserves_existing_pem_text() {
        let raw = b"-----BEGIN PUBLIC KEY-----\r\nQUJD\n-----END PUBLIC KEY-----\r\n";
        let pem = normalize_public_key_pem(raw);
        assert_eq!(
            pem,
            "-----BEGIN PUBLIC KEY-----\nQUJD\n-----END PUBLIC KEY-----\n"
        );
    }
}
