use std::{
    collections::HashMap,
    fmt,
    sync::{OnceLock, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use aes_gcm::{
    aead::{Aead, AeadCore, OsRng, Payload},
    Aes256Gcm, KeyInit,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const PACKAGE_MAGIC: &[u8; 8] = b"XYPCAT01";
const PACKAGE_FORMAT_VERSION: u16 = 1;
const NONCE_LEN: usize = 12;
const AAD_DIGEST_LEN: usize = 32;
const HEADER_LEN: usize = PACKAGE_MAGIC.len() + 2 + NONCE_LEN + AAD_DIGEST_LEN;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/protocol_catalog_generated.rs"));
}

pub use generated::opcodes;
pub use generated::BUILTIN_PROTOCOL_CAPABILITY;

static BUILTIN_CATALOG: OnceLock<RwLock<Option<LoadedCatalog>>> = OnceLock::new();

struct LoadedCatalog {
    catalog: ProtocolCatalog,
    expires_at: SystemTime,
}

pub struct ProtocolKey([u8; 32]);

impl ProtocolKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for ProtocolKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogContext {
    app_version: String,
    module: String,
}

impl CatalogContext {
    pub fn new(app_version: impl Into<String>, module: impl Into<String>) -> Self {
        Self {
            app_version: app_version.into(),
            module: module.into(),
        }
    }

    fn associated_data(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.app_version.len() + self.module.len() + 8);
        append_length_prefixed(&mut output, self.app_version.as_bytes());
        append_length_prefixed(&mut output, self.module.as_bytes());
        output
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProtocolEntry {
    opcode: u32,
    gateway: String,
    api: String,
    version: String,
    method: String,
    headers_profile: Option<String>,
    params_template: Value,
}

impl ProtocolEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        opcode: u32,
        gateway: impl Into<String>,
        api: impl Into<String>,
        version: impl Into<String>,
        method: impl Into<String>,
        headers_profile: Option<String>,
        params_template: Value,
    ) -> Self {
        Self {
            opcode,
            gateway: gateway.into(),
            api: api.into(),
            version: version.into(),
            method: method.into(),
            headers_profile,
            params_template,
        }
    }

    pub fn opcode(&self) -> u32 {
        self.opcode
    }

    pub fn gateway(&self) -> &str {
        &self.gateway
    }

    pub fn api(&self) -> &str {
        &self.api
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn headers_profile(&self) -> Option<&str> {
        self.headers_profile.as_deref()
    }

    pub fn params_template(&self) -> &Value {
        &self.params_template
    }
}

impl fmt::Debug for ProtocolEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtocolEntry")
            .field("opcode", &self.opcode)
            .field("redacted", &true)
            .finish()
    }
}

impl Drop for ProtocolEntry {
    fn drop(&mut self) {
        self.gateway.zeroize();
        self.api.zeroize();
        self.version.zeroize();
        self.method.zeroize();
        if let Some(profile) = self.headers_profile.as_mut() {
            profile.zeroize();
        }
        zeroize_json_value(&mut self.params_template);
    }
}

#[derive(Serialize, Deserialize)]
pub struct CatalogDocument {
    entries: Vec<ProtocolEntry>,
}

impl CatalogDocument {
    pub fn new(entries: Vec<ProtocolEntry>) -> Self {
        Self { entries }
    }
}

pub struct ProtocolCatalog {
    entries: HashMap<u32, ProtocolEntry>,
}

impl ProtocolCatalog {
    pub fn get(&self, opcode: u32) -> Result<&ProtocolEntry, ProtocolCatalogError> {
        self.entries
            .get(&opcode)
            .ok_or(ProtocolCatalogError::UnknownOpcode { opcode })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BuiltinCatalogStatus {
    loaded: bool,
    entry_count: usize,
    expires_at_unix_seconds: Option<u64>,
}

impl BuiltinCatalogStatus {
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    pub fn expires_at_unix_seconds(&self) -> Option<u64> {
        self.expires_at_unix_seconds
    }
}

impl fmt::Debug for ProtocolCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtocolCatalog")
            .field("entry_count", &self.entries.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ProtocolCatalogError {
    MalformedPackage,
    InvalidMagic,
    UnsupportedFormatVersion { found: u16 },
    AssociatedDataMismatch,
    AuthenticationFailed,
    Serialization,
    DuplicateOpcode { opcode: u32 },
    UnknownOpcode { opcode: u32 },
    CatalogNotLoaded,
    CatalogExpired,
    RuntimeStatePoisoned,
}

impl fmt::Display for ProtocolCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedPackage => formatter.write_str("malformed protocol catalog package"),
            Self::InvalidMagic => formatter.write_str("invalid protocol catalog package magic"),
            Self::UnsupportedFormatVersion { found } => {
                write!(
                    formatter,
                    "unsupported protocol catalog format version: {found}"
                )
            }
            Self::AssociatedDataMismatch => {
                formatter.write_str("protocol catalog associated data mismatch")
            }
            Self::AuthenticationFailed => {
                formatter.write_str("protocol catalog authentication failed")
            }
            Self::Serialization => formatter.write_str("protocol catalog serialization failed"),
            Self::DuplicateOpcode { opcode } => {
                write!(formatter, "duplicate protocol catalog opcode: {opcode}")
            }
            Self::UnknownOpcode { opcode } => {
                write!(formatter, "unknown protocol catalog opcode: {opcode}")
            }
            Self::CatalogNotLoaded => formatter.write_str("protocol catalog is not loaded"),
            Self::CatalogExpired => formatter.write_str("protocol catalog has expired"),
            Self::RuntimeStatePoisoned => {
                formatter.write_str("protocol catalog runtime state is poisoned")
            }
        }
    }
}

impl std::error::Error for ProtocolCatalogError {}

pub fn builtin_catalog_context() -> CatalogContext {
    CatalogContext::new(env!("CARGO_PKG_VERSION"), BUILTIN_PROTOCOL_CAPABILITY)
}

pub fn community_protocol_key() -> ProtocolKey {
    ProtocolKey::new(generated::community_protocol_key_bytes())
}

pub fn install_builtin_catalog(
    key: &ProtocolKey,
    expires_at: SystemTime,
) -> Result<(), ProtocolCatalogError> {
    if expires_at <= SystemTime::now() {
        clear_builtin_catalog()?;
        return Err(ProtocolCatalogError::CatalogExpired);
    }

    let catalog = decrypt_catalog(
        generated::BUILTIN_PROTOCOL_PACKAGE,
        key,
        &builtin_catalog_context(),
    )?;
    let mut state = builtin_catalog_state()
        .write()
        .map_err(|_| ProtocolCatalogError::RuntimeStatePoisoned)?;
    *state = Some(LoadedCatalog {
        catalog,
        expires_at,
    });
    Ok(())
}

pub fn resolve_builtin_protocol(opcode: u32) -> Result<ProtocolEntry, ProtocolCatalogError> {
    let mut state = builtin_catalog_state()
        .write()
        .map_err(|_| ProtocolCatalogError::RuntimeStatePoisoned)?;
    let Some(loaded) = state.as_ref() else {
        return Err(ProtocolCatalogError::CatalogNotLoaded);
    };
    if loaded.expires_at <= SystemTime::now() {
        *state = None;
        return Err(ProtocolCatalogError::CatalogExpired);
    }
    loaded.catalog.get(opcode).cloned()
}

pub fn clear_builtin_catalog() -> Result<(), ProtocolCatalogError> {
    let mut state = builtin_catalog_state()
        .write()
        .map_err(|_| ProtocolCatalogError::RuntimeStatePoisoned)?;
    *state = None;
    Ok(())
}

pub fn builtin_catalog_status() -> Result<BuiltinCatalogStatus, ProtocolCatalogError> {
    let mut state = builtin_catalog_state()
        .write()
        .map_err(|_| ProtocolCatalogError::RuntimeStatePoisoned)?;
    if state
        .as_ref()
        .is_some_and(|loaded| loaded.expires_at <= SystemTime::now())
    {
        *state = None;
    }

    let Some(loaded) = state.as_ref() else {
        return Ok(BuiltinCatalogStatus {
            loaded: false,
            entry_count: 0,
            expires_at_unix_seconds: None,
        });
    };
    Ok(BuiltinCatalogStatus {
        loaded: true,
        entry_count: loaded.catalog.len(),
        expires_at_unix_seconds: loaded
            .expires_at
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs()),
    })
}

fn builtin_catalog_state() -> &'static RwLock<Option<LoadedCatalog>> {
    BUILTIN_CATALOG.get_or_init(|| RwLock::new(None))
}

pub fn encrypt_catalog(
    document: &CatalogDocument,
    key: &ProtocolKey,
    context: &CatalogContext,
) -> Result<Vec<u8>, ProtocolCatalogError> {
    let mut plaintext =
        serde_json::to_vec(document).map_err(|_| ProtocolCatalogError::Serialization)?;
    let result = encrypt_plaintext(&plaintext, key, context);
    plaintext.zeroize();
    result
}

pub fn decrypt_catalog(
    package: &[u8],
    key: &ProtocolKey,
    context: &CatalogContext,
) -> Result<ProtocolCatalog, ProtocolCatalogError> {
    let (nonce, expected_aad_digest, ciphertext) = parse_package(package)?;
    let context_aad = context.associated_data();
    let actual_aad_digest = Sha256::digest(&context_aad);
    if actual_aad_digest.as_slice() != expected_aad_digest {
        return Err(ProtocolCatalogError::AssociatedDataMismatch);
    }

    let aad = package_aad(&package[..HEADER_LEN], &context_aad);
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
        .map_err(|_| ProtocolCatalogError::AuthenticationFailed)?;
    let mut plaintext = cipher
        .decrypt(
            nonce.into(),
            Payload {
                msg: ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| ProtocolCatalogError::AuthenticationFailed)?;

    let result = deserialize_catalog(&plaintext);
    plaintext.zeroize();
    result
}

fn encrypt_plaintext(
    plaintext: &[u8],
    key: &ProtocolKey,
    context: &CatalogContext,
) -> Result<Vec<u8>, ProtocolCatalogError> {
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
        .map_err(|_| ProtocolCatalogError::AuthenticationFailed)?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let context_aad = context.associated_data();
    let aad_digest = Sha256::digest(&context_aad);

    let mut package = Vec::with_capacity(HEADER_LEN + plaintext.len() + 16);
    package.extend_from_slice(PACKAGE_MAGIC);
    package.extend_from_slice(&PACKAGE_FORMAT_VERSION.to_be_bytes());
    package.extend_from_slice(&nonce);
    package.extend_from_slice(&aad_digest);

    let aad = package_aad(&package, &context_aad);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| ProtocolCatalogError::AuthenticationFailed)?;
    package.extend_from_slice(&ciphertext);
    Ok(package)
}

fn parse_package(package: &[u8]) -> Result<(&[u8], &[u8], &[u8]), ProtocolCatalogError> {
    if package.len() <= HEADER_LEN {
        return Err(ProtocolCatalogError::MalformedPackage);
    }
    if &package[..PACKAGE_MAGIC.len()] != PACKAGE_MAGIC {
        return Err(ProtocolCatalogError::InvalidMagic);
    }

    let version_offset = PACKAGE_MAGIC.len();
    let version = u16::from_be_bytes([package[version_offset], package[version_offset + 1]]);
    if version != PACKAGE_FORMAT_VERSION {
        return Err(ProtocolCatalogError::UnsupportedFormatVersion { found: version });
    }

    let nonce_start = version_offset + 2;
    let nonce_end = nonce_start + NONCE_LEN;
    let digest_end = nonce_end + AAD_DIGEST_LEN;
    Ok((
        &package[nonce_start..nonce_end],
        &package[nonce_end..digest_end],
        &package[digest_end..],
    ))
}

fn deserialize_catalog(plaintext: &[u8]) -> Result<ProtocolCatalog, ProtocolCatalogError> {
    let document: CatalogDocument =
        serde_json::from_slice(plaintext).map_err(|_| ProtocolCatalogError::Serialization)?;
    let mut entries = HashMap::with_capacity(document.entries.len());
    for entry in document.entries {
        let opcode = entry.opcode;
        if entries.insert(opcode, entry).is_some() {
            return Err(ProtocolCatalogError::DuplicateOpcode { opcode });
        }
    }
    Ok(ProtocolCatalog { entries })
}

fn package_aad(header: &[u8], context_aad: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(header.len() + context_aad.len());
    aad.extend_from_slice(header);
    aad.extend_from_slice(context_aad);
    aad
}

fn append_length_prefixed(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).unwrap_or(u32::MAX);
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn zeroize_json_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(values) => values.iter_mut().for_each(zeroize_json_value),
        Value::Object(values) => {
            for (_, value) in values.iter_mut() {
                zeroize_json_value(value);
            }
            values.clear();
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
