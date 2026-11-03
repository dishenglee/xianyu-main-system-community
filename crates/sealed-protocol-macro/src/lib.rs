use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use proc_macro::{TokenStream, TokenTree};
use std::str::FromStr;

const SEALED_KEY: [u8; 32] = [0x6d; 32];

#[proc_macro]
pub fn sealed_str(input: TokenStream) -> TokenStream {
    let mut tokens = input.into_iter();
    let literal = match (tokens.next(), tokens.next()) {
        (Some(TokenTree::Literal(literal)), None) => literal.to_string(),
        _ => return compile_error("sealed_str! expects exactly one string literal"),
    };
    let plaintext = match decode_literal(&literal) {
        Ok(value) => value,
        Err(error) => return compile_error(error),
    };
    let seed = fnv1a64(plaintext.as_bytes()) ^ 0x9E37_79B9_7F4A_7C15;
    let nonce = nonce_from_seed(seed);
    let cipher = Aes256Gcm::new_from_slice(&SEALED_KEY).expect("sealed key length");
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .expect("sealed protocol literal encryption must succeed");
    let nonce_bytes = nonce
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let bytes = encrypted
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    TokenStream::from_str(&format!(
        "{{ static VALUE: ::std::sync::OnceLock<::std::string::String> = ::std::sync::OnceLock::new(); VALUE.get_or_init(|| ::xianyu_rust_backend_full::sealed_protocol::decode(&[{bytes}], &[{nonce_bytes}])).as_str() }}"
    ))
    .unwrap_or_else(|_| compile_error("sealed_str! expansion failed"))
}

fn decode_literal(literal: &str) -> Result<String, &'static str> {
    if literal.starts_with('r') {
        return decode_raw_literal(literal);
    }
    let inner = literal
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or("sealed_str! requires a string literal")?;
    let mut output = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match chars.next().ok_or("sealed_str! has an incomplete escape")? {
            '\\' => output.push('\\'),
            '"' => output.push('"'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            '0' => output.push('\0'),
            _ => return Err("sealed_str! encountered an unsupported escape"),
        }
    }
    Ok(output)
}

fn decode_raw_literal(literal: &str) -> Result<String, &'static str> {
    let first_quote = literal.find('"').ok_or("invalid raw string literal")?;
    let hashes = &literal[1..first_quote];
    if !hashes.chars().all(|character| character == '#') {
        return Err("invalid raw string literal");
    }
    let suffix = format!("\"{hashes}");
    let body = literal[first_quote + 1..]
        .strip_suffix(&suffix)
        .ok_or("invalid raw string literal")?;
    Ok(body.to_string())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn nonce_from_seed(seed: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..8].copy_from_slice(&seed.to_le_bytes());
    nonce[8..].copy_from_slice(&(seed.rotate_left(17) as u32).to_le_bytes());
    nonce
}

fn compile_error(message: &str) -> TokenStream {
    TokenStream::from_str(&format!("compile_error!({message:?})")).unwrap()
}
