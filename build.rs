use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

use aes_gcm::{
    aead::{Aead, AeadCore, OsRng, Payload},
    Aes256Gcm, KeyInit,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const PACKAGE_MAGIC: &[u8; 8] = b"XYPCAT01";
const PACKAGE_FORMAT_VERSION: u16 = 1;
const DEVELOPMENT_TEST_KEY: [u8; 32] = [0xD7; 32];

fn main() {
    for path in [
        "migrations/accounts",
        "migrations/xianyu",
        "migrations/live_state",
        "migrations/notifications",
    ] {
        println!("cargo:rerun-if-changed={path}");
        track_migration_files(Path::new(path));
    }

    build_protocol_catalog();
}

// `sqlx::migrate!` embeds migration contents during macro expansion, but Cargo
// does not otherwise know that every SQL file is a Rust compilation input.
// Track them explicitly so a rebuilt test App never retains an old migrator.
fn track_migration_files(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogDefinition {
    schema_version: u16,
    capability: String,
    entries: Vec<BuildProtocolEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFragment {
    entries: Vec<BuildProtocolEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildProtocolEntry {
    name: String,
    opcode: u32,
    gateway: String,
    api: String,
    version: String,
    method: String,
    headers_profile: Option<String>,
    params_template: serde_json::Value,
}

#[derive(Serialize)]
struct WireCatalog<'a> {
    entries: Vec<WireProtocolEntry<'a>>,
}

#[derive(Serialize)]
struct WireProtocolEntry<'a> {
    opcode: u32,
    gateway: &'a str,
    api: &'a str,
    version: &'a str,
    method: &'a str,
    headers_profile: Option<&'a str>,
    params_template: &'a serde_json::Value,
}

fn build_protocol_catalog() {
    const DEFINITION_PATH: &str = "protocol/catalog.json";
    println!("cargo:rerun-if-changed={DEFINITION_PATH}");

    let raw = fs::read_to_string(DEFINITION_PATH)
        .unwrap_or_else(|error| panic!("read {DEFINITION_PATH}: {error}"));
    let mut definition: CatalogDefinition = serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("parse {DEFINITION_PATH}: {error}"));
    load_protocol_fragments(&mut definition);
    validate_definition(&definition);

    let mut key = load_protocol_key();
    let app_version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is required");
    let wire_catalog = WireCatalog {
        entries: definition
            .entries
            .iter()
            .map(|entry| WireProtocolEntry {
                opcode: entry.opcode,
                gateway: &entry.gateway,
                api: &entry.api,
                version: &entry.version,
                method: &entry.method,
                headers_profile: entry.headers_profile.as_deref(),
                params_template: &entry.params_template,
            })
            .collect(),
    };
    let mut plaintext = serde_json::to_vec(&wire_catalog).expect("serialize protocol catalog");
    let package = encrypt_protocol_package(
        &plaintext,
        &key,
        &app_version,
        definition.capability.as_str(),
    );
    plaintext.zeroize();
    key.zeroize();

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is required"));
    fs::write(out_dir.join("protocol_catalog.bundle"), package)
        .expect("write encrypted protocol catalog bundle");
    fs::write(
        out_dir.join("protocol_catalog_generated.rs"),
        generated_rust(&definition),
    )
    .expect("write generated protocol catalog Rust module");
}

fn load_protocol_fragments(definition: &mut CatalogDefinition) {
    const FRAGMENT_DIR: &str = "protocol/catalog.d";
    println!("cargo:rerun-if-changed={FRAGMENT_DIR}");
    let Ok(entries) = fs::read_dir(FRAGMENT_DIR) else {
        return;
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read protocol fragment {}: {error}", path.display()));
        let mut fragment: CatalogFragment = serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("parse protocol fragment {}: {error}", path.display()));
        definition.entries.append(&mut fragment.entries);
    }
}

fn validate_definition(definition: &CatalogDefinition) {
    if definition.schema_version != 1 {
        panic!(
            "unsupported protocol catalog definition schema version: {}",
            definition.schema_version
        );
    }
    if definition.capability.trim().is_empty() {
        panic!("protocol catalog capability must not be empty");
    }

    let mut names = HashSet::new();
    let mut opcodes = HashSet::new();
    for entry in &definition.entries {
        if !valid_constant_name(&entry.name) {
            panic!("invalid protocol opcode constant name: {}", entry.name);
        }
        if !names.insert(entry.name.as_str()) {
            panic!("duplicate protocol opcode constant name: {}", entry.name);
        }
        if !opcodes.insert(entry.opcode) {
            panic!("duplicate protocol opcode: {}", entry.opcode);
        }
        if entry.gateway.trim().is_empty()
            || entry.api.trim().is_empty()
            || entry.version.trim().is_empty()
            || entry.method.trim().is_empty()
        {
            panic!("protocol entry {} has an empty required field", entry.name);
        }
    }
}

fn valid_constant_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('A'..='Z'))
        && chars.all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
}

fn load_protocol_key() -> [u8; 32] {
    DEVELOPMENT_TEST_KEY
}

fn encrypt_protocol_package(
    plaintext: &[u8],
    key: &[u8; 32],
    app_version: &str,
    capability: &str,
) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(key).expect("AES-256-GCM key length");
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let context = associated_data(app_version, capability);
    let digest = Sha256::digest(&context);

    let mut package = Vec::with_capacity(8 + 2 + 12 + 32 + plaintext.len() + 16);
    package.extend_from_slice(PACKAGE_MAGIC);
    package.extend_from_slice(&PACKAGE_FORMAT_VERSION.to_be_bytes());
    package.extend_from_slice(&nonce);
    package.extend_from_slice(&digest);

    let mut aad = Vec::with_capacity(package.len() + context.len());
    aad.extend_from_slice(&package);
    aad.extend_from_slice(&context);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .expect("encrypt protocol catalog");
    aad.zeroize();
    package.extend_from_slice(&ciphertext);
    package
}

fn associated_data(app_version: &str, capability: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(app_version.len() + capability.len() + 8);
    append_length_prefixed(&mut output, app_version.as_bytes());
    append_length_prefixed(&mut output, capability.as_bytes());
    output
}

fn append_length_prefixed(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).expect("protocol context field exceeds u32");
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn generated_rust(definition: &CatalogDefinition) -> String {
    let mut output = String::new();
    output.push_str("pub const BUILTIN_PROTOCOL_CAPABILITY: &str = ");
    output.push_str(&format!("{:?};\n", definition.capability));
    output.push_str(
        "pub static BUILTIN_PROTOCOL_PACKAGE: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/protocol_catalog.bundle\"));\n",
    );
    output.push_str("pub mod opcodes {\n");
    for entry in &definition.entries {
        output.push_str(&format!(
            "    pub const {}: u32 = {};\n",
            entry.name, entry.opcode
        ));
    }
    output.push_str("}\n");
    output.push_str("pub fn community_protocol_key_bytes() -> [u8; 32] { [0xD7; 32] }\n");
    output
}
