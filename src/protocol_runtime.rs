use std::{sync::OnceLock, time::Duration};

use tokio::sync::Mutex;

use crate::{
    protocol_catalog::{
        community_protocol_key, install_builtin_catalog, resolve_builtin_protocol,
        ProtocolCatalogError, ProtocolEntry,
    },
    state::AppState,
};

fn unlock_guard() -> &'static Mutex<()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD.get_or_init(|| Mutex::new(()))
}

pub async fn resolve_protocol(_state: &AppState, opcode: u32) -> Result<ProtocolEntry, String> {
    resolve_local_protocol(opcode).await
}

async fn resolve_local_protocol(opcode: u32) -> Result<ProtocolEntry, String> {
    match resolve_builtin_protocol(opcode) {
        Ok(entry) => return Ok(entry),
        Err(ProtocolCatalogError::CatalogNotLoaded | ProtocolCatalogError::CatalogExpired) => {}
        Err(error) => return Err(error.to_string()),
    }

    let _guard = unlock_guard().lock().await;
    if let Ok(entry) = resolve_builtin_protocol(opcode) {
        return Ok(entry);
    }

    let key = community_protocol_key();
    let expires_at = std::time::SystemTime::now() + Duration::from_secs(100 * 365 * 24 * 60 * 60);
    install_builtin_catalog(&key, expires_at).map_err(|error| error.to_string())?;
    resolve_builtin_protocol(opcode).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_embedded_protocol_without_license_context_or_network() {
        crate::protocol_catalog::clear_builtin_catalog().unwrap();

        let entry = resolve_local_protocol(crate::protocol_catalog::opcodes::SEARCH_APP)
            .await
            .unwrap();

        assert!(!entry.api().trim().is_empty());
    }
}
