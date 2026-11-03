use aes_gcm::{aead::Aead, KeyInit};

pub use sealed_protocol_macro::sealed_str;

#[doc(hidden)]
#[inline(never)]
pub fn decode(encrypted: &[u8], nonce_bytes: &[u8]) -> String {
    // Keep the decrypt boundary opaque to release/LTO constant propagation.
    // Otherwise a fully-known sealed literal can be folded back into plaintext
    // in a linked Tauri Mach-O even though the source only embeds ciphertext.
    let encrypted = std::hint::black_box(encrypted);
    let nonce_bytes = std::hint::black_box(nonce_bytes);
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&[0x6d; 32]).expect("sealed key length");
    assert_eq!(nonce_bytes.len(), 12, "sealed nonce length");
    let plaintext = cipher
        .decrypt(aes_gcm::Nonce::from_slice(&nonce_bytes), encrypted)
        .expect("sealed protocol literal authentication failed");
    String::from_utf8(plaintext).expect("sealed protocol literal must remain valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::sealed_str;

    #[test]
    fn sealed_protocol_literal_round_trips() {
        assert_eq!(
            sealed_str!("example.protocol.operation"),
            "example.protocol.operation"
        );
    }
}
