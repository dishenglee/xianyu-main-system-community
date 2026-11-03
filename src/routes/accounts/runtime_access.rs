use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde_json::{Map, Value};
use tracing::warn;

use crate::{license_runtime::get_sign_license_context, state::AppState};

use super::{
    cloud_binding::{claim_binding, resolve_bindings, ClaimBindingError},
    license_server_url, ResponseError,
};

pub(crate) const ACCOUNT_DELETE_ONLY_MESSAGE: &str = "此账号已被其他用户提交绑定，只能进行删除操作";
const RUNTIME_ACCESS_CACHE_TTL: Duration = Duration::from_secs(30);
const RUNTIME_ACCESS_DELETE_ONLY_CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuntimeAccessCacheKey {
    server_url: String,
    license_id: i64,
    account_uid: String,
}

#[derive(Debug, Clone)]
struct RuntimeAccessCacheEntry {
    access: AccountRuntimeAccess,
    cached_at: Instant,
}

fn runtime_access_cache() -> &'static Mutex<HashMap<RuntimeAccessCacheKey, RuntimeAccessCacheEntry>>
{
    static CACHE: OnceLock<Mutex<HashMap<RuntimeAccessCacheKey, RuntimeAccessCacheEntry>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AccountRuntimeAccess {
    pub(crate) remote_binding_state: String,
    pub(crate) remote_binding_message: String,
    pub(crate) delete_only: bool,
}

fn should_delete_only(status: &str) -> bool {
    matches!(status, "owned_by_other")
}

fn should_reclaim_local_account(status: &str) -> bool {
    matches!(status.trim(), "released_from_me" | "available")
}

fn runtime_access_from_resolution(status: &str, message: &str) -> AccountRuntimeAccess {
    let remote_binding_state = status.trim().to_string();
    let delete_only = should_delete_only(remote_binding_state.as_str());
    let remote_binding_message = if delete_only {
        ACCOUNT_DELETE_ONLY_MESSAGE.to_string()
    } else {
        message.trim().to_string()
    };
    AccountRuntimeAccess {
        remote_binding_state,
        remote_binding_message,
        delete_only,
    }
}

fn runtime_access_cache_key(
    context: &crate::license_runtime::SignLicenseContext,
    state: &AppState,
    uid: &str,
) -> Option<RuntimeAccessCacheKey> {
    let server_url = license_server_url(state)
        .trim()
        .trim_end_matches('/')
        .to_string();
    if server_url.is_empty() || uid.trim().is_empty() {
        return None;
    }
    Some(RuntimeAccessCacheKey {
        server_url,
        license_id: context.license_id,
        account_uid: uid.trim().to_string(),
    })
}

fn cached_runtime_access(key: &RuntimeAccessCacheKey) -> Option<AccountRuntimeAccess> {
    let mut cache = runtime_access_cache().lock().ok()?;
    let entry = cache.get(key)?;
    let ttl = if entry.access.delete_only {
        RUNTIME_ACCESS_DELETE_ONLY_CACHE_TTL
    } else {
        RUNTIME_ACCESS_CACHE_TTL
    };
    if entry.cached_at.elapsed() >= ttl {
        cache.remove(key);
        return None;
    }
    Some(entry.access.clone())
}

fn store_runtime_access_cache(key: RuntimeAccessCacheKey, access: AccountRuntimeAccess) {
    if let Ok(mut cache) = runtime_access_cache().lock() {
        if cache.len() > 2048 {
            cache.retain(|_, entry| {
                let ttl = if entry.access.delete_only {
                    RUNTIME_ACCESS_DELETE_ONLY_CACHE_TTL
                } else {
                    RUNTIME_ACCESS_CACHE_TTL
                };
                entry.cached_at.elapsed() < ttl
            });
        }
        cache.insert(
            key,
            RuntimeAccessCacheEntry {
                access,
                cached_at: Instant::now(),
            },
        );
    }
}

pub(crate) fn attach_runtime_access_fields(
    target: &mut Map<String, Value>,
    access: &AccountRuntimeAccess,
) {
    target.insert(
        "remote_binding_state".to_string(),
        Value::String(access.remote_binding_state.clone()),
    );
    target.insert(
        "remoteBindingState".to_string(),
        Value::String(access.remote_binding_state.clone()),
    );
    target.insert(
        "remote_binding_message".to_string(),
        Value::String(access.remote_binding_message.clone()),
    );
    target.insert(
        "remoteBindingMessage".to_string(),
        Value::String(access.remote_binding_message.clone()),
    );
    target.insert("delete_only".to_string(), Value::Bool(access.delete_only));
    target.insert("deleteOnly".to_string(), Value::Bool(access.delete_only));
}

pub(crate) async fn load_account_owner_username(
    state: &AppState,
    account_uid: &str,
) -> Result<Option<String>, ResponseError> {
    sqlx::query_scalar::<_, String>("SELECT owner_username FROM accounts WHERE uid = ? LIMIT 1")
        .bind(account_uid.trim())
        .fetch_optional(state.storage().accounts())
        .await
        .map_err(ResponseError::internal)
}

async fn load_account_nick_for_binding_claim(
    state: &AppState,
    account_uid: &str,
) -> Result<String, ResponseError> {
    let nick = sqlx::query_scalar::<_, String>("SELECT nick FROM accounts WHERE uid = ? LIMIT 1")
        .bind(account_uid.trim())
        .fetch_optional(state.storage().accounts())
        .await
        .map_err(ResponseError::internal)?
        .unwrap_or_else(|| account_uid.trim().to_string());
    Ok(nick.trim().to_string())
}

async fn reclaim_local_account_binding_if_needed(
    state: &AppState,
    auth_token: &str,
    account_uid: &str,
    status: &str,
) -> Result<Option<AccountRuntimeAccess>, ResponseError> {
    if !should_reclaim_local_account(status) {
        return Ok(None);
    }

    let nick = load_account_nick_for_binding_claim(state, account_uid).await?;
    match claim_binding(state, auth_token, account_uid, &nick).await {
        Ok(()) => Ok(Some(runtime_access_from_resolution(
            "owned_by_me",
            "该闲鱼账号当前归属于你",
        ))),
        Err(ClaimBindingError::OwnerConflict(message)) => {
            warn!(
                module = "account-runtime-access",
                action = "reclaim_local_account_binding",
                account_uid = %account_uid,
                status = %status,
                detail = %message,
                "local account binding reclaim hit owner conflict"
            );
            Ok(Some(runtime_access_from_resolution(
                "owned_by_other",
                &message,
            )))
        }
        Err(ClaimBindingError::Other(error)) => {
            warn!(
                module = "account-runtime-access",
                action = "reclaim_local_account_binding",
                account_uid = %account_uid,
                status = %status,
                detail = %error.detail(),
                "local account binding reclaim failed"
            );
            Ok(None)
        }
    }
}

pub(crate) async fn resolve_account_runtime_access_with_auth_token(
    state: &AppState,
    auth_token: &str,
    account_uid: &str,
) -> Result<AccountRuntimeAccess, ResponseError> {
    let mut resolved =
        resolve_bindings(state, auth_token, &[account_uid.trim().to_string()]).await?;
    let Some(binding) = resolved.pop() else {
        return Ok(AccountRuntimeAccess::default());
    };
    if let Some(access) = reclaim_local_account_binding_if_needed(
        state,
        auth_token,
        account_uid,
        binding.status.as_str(),
    )
    .await?
    {
        return Ok(access);
    }
    Ok(runtime_access_from_resolution(
        binding.status.as_str(),
        binding.message.as_str(),
    ))
}

pub(crate) async fn resolve_accounts_runtime_access(
    state: &AppState,
    account_uids: &[String],
) -> Result<HashMap<String, AccountRuntimeAccess>, ResponseError> {
    let Some(context) = get_sign_license_context() else {
        return Ok(HashMap::new());
    };
    let auth_token = context.auth_token.trim();
    if auth_token.is_empty() || license_server_url(state).is_empty() {
        return Ok(HashMap::new());
    }

    let mut access_by_uid = HashMap::new();
    let mut missing_uids = Vec::new();
    let mut cache_keys = HashMap::new();
    for raw_uid in account_uids {
        let uid = raw_uid.trim();
        if uid.is_empty() {
            continue;
        }
        let Some(key) = runtime_access_cache_key(&context, state, uid) else {
            missing_uids.push(uid.to_string());
            continue;
        };
        if let Some(access) = cached_runtime_access(&key) {
            access_by_uid.insert(uid.to_string(), access);
        } else {
            cache_keys.insert(uid.to_string(), key);
            missing_uids.push(uid.to_string());
        }
    }

    if missing_uids.is_empty() {
        return Ok(access_by_uid);
    }

    let resolved = resolve_bindings(state, auth_token, &missing_uids).await?;
    let mut resolved_by_uid = HashMap::new();
    for binding in resolved {
        let account_uid = binding.account_uid.trim().to_string();
        let access = match reclaim_local_account_binding_if_needed(
            state,
            auth_token,
            &account_uid,
            binding.status.as_str(),
        )
        .await?
        {
            Some(access) => access,
            None => {
                runtime_access_from_resolution(binding.status.as_str(), binding.message.as_str())
            }
        };
        resolved_by_uid.insert(account_uid, access);
    }

    for uid in missing_uids {
        let access = resolved_by_uid.remove(&uid).unwrap_or_default();
        if let Some(key) = cache_keys.remove(&uid) {
            store_runtime_access_cache(key, access.clone());
        }
        access_by_uid.insert(uid, access);
    }

    Ok(access_by_uid)
}

pub(crate) async fn ensure_account_can_call_upstream(
    state: &AppState,
    _owner_username: Option<&str>,
    account_uid: &str,
) -> Result<(), ResponseError> {
    let uid = account_uid.trim();
    if uid.is_empty() {
        return Err(ResponseError::bad_request("缺少账号 uid"));
    }
    let Some(context) = get_sign_license_context() else {
        return Ok(());
    };
    let auth_token = context.auth_token.trim();
    if auth_token.is_empty() || license_server_url(state).is_empty() {
        return Ok(());
    }
    let cache_key = runtime_access_cache_key(&context, state, uid);
    if let Some(access) = cache_key.as_ref().and_then(cached_runtime_access) {
        if access.delete_only {
            return Err(ResponseError::forbidden(access.remote_binding_message));
        }
        return Ok(());
    }
    let access = resolve_account_runtime_access_with_auth_token(state, auth_token, uid).await?;
    if let Some(key) = cache_key {
        store_runtime_access_cache(key, access.clone());
    }
    if access.delete_only {
        return Err(ResponseError::forbidden(access.remote_binding_message));
    }
    Ok(())
}
