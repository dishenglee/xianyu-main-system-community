use reqwest::{tls::TlsInfo, Response};
use sha2::{Digest, Sha256};

pub fn parse_pin_set(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|value| normalize_pin(value).unwrap_or_default())
        .filter(|value| !value.is_empty())
        .collect()
}

pub fn normalize_pin(raw: &str) -> Result<String, String> {
    let compact = raw
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .collect::<String>()
        .to_lowercase();
    if compact.is_empty() {
        return Ok(String::new());
    }
    if compact.len() != 64 {
        return Err(format!(
            "pin must be 64 hex chars after normalization, got {}",
            compact.len()
        ));
    }
    Ok(compact)
}

pub fn fingerprint_hex(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn verify_response_pin(
    response: &Response,
    expected_pins: &[String],
    label: &str,
    url: &str,
) -> Result<(), String> {
    if expected_pins.is_empty() {
        return Ok(());
    }

    let Some(info) = response.extensions().get::<TlsInfo>() else {
        return Err(format!("{label} TLS info missing for {url}"));
    };
    let Some(cert) = info.peer_certificate() else {
        return Err(format!("{label} peer certificate missing for {url}"));
    };

    let actual = fingerprint_hex(cert);
    if expected_pins.iter().any(|pin| pin == &actual) {
        return Ok(());
    }

    Err(format!(
        "{label} certificate pin mismatch for {url}: got {actual}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_pin_strips_separators() {
        let raw = format!("{}:{}", "AA".repeat(16), "bb".repeat(16));
        let value = normalize_pin(&raw).unwrap();
        assert_eq!(value, format!("{}{}", "aa".repeat(16), "bb".repeat(16)));
    }

    #[test]
    fn parse_pin_set_ignores_empty_entries() {
        let pin_a = "11".repeat(32);
        let pin_b = "22".repeat(32);
        let pins = parse_pin_set(&format!(" {pin_a} , , {pin_b} "));
        assert_eq!(pins, vec![pin_a, pin_b]);
    }

    #[test]
    fn fingerprint_hex_is_stable() {
        let actual = fingerprint_hex(b"hello");
        assert_eq!(
            actual,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
