use std::collections::HashSet;

use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::{
    models::auth::{AuthUser, RequestIdentity},
    routes::error::ResponseError,
    state::AppState,
};

pub(crate) fn owner_username_filter(
    _state: &AppState,
    identity: &RequestIdentity,
) -> Option<String> {
    if has_unscoped_legacy_access(identity) {
        None
    } else {
        Some(identity.username.trim().to_string())
    }
}

pub(crate) fn is_admin_or_legacy(_state: &AppState, identity: &RequestIdentity) -> bool {
    is_admin_or_legacy_values(&identity.role, identity.legacy)
}

fn is_admin_or_legacy_values(role: &str, legacy: bool) -> bool {
    legacy || role.eq_ignore_ascii_case("admin")
}

pub(crate) fn has_unscoped_legacy_access(identity: &RequestIdentity) -> bool {
    identity.legacy
}

pub(crate) fn auth_user_has_unscoped_legacy_access(user: &AuthUser) -> bool {
    user.legacy
}

pub(crate) fn ensure_admin_or_legacy(
    state: &AppState,
    identity: &RequestIdentity,
) -> Result<(), ResponseError> {
    if is_admin_or_legacy(state, identity) {
        Ok(())
    } else {
        Err(ResponseError::forbidden("仅管理员可使用该功能"))
    }
}

pub(crate) async fn accessible_account_uids(
    pool: &SqlitePool,
    owner_username: Option<&str>,
) -> Result<HashSet<String>, sqlx::Error> {
    let rows = if let Some(owner) = owner_username {
        sqlx::query("SELECT uid FROM accounts WHERE owner_username = ? AND status != 'disabled'")
            .bind(owner)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query("SELECT uid FROM accounts WHERE status != 'disabled'")
            .fetch_all(pool)
            .await?
    };

    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("uid").ok())
        .collect())
}

pub(crate) async fn ensure_account_uids_accessible(
    pool: &SqlitePool,
    owner_username: Option<&str>,
    account_uids: &[String],
) -> Result<(), ResponseError> {
    if account_uids.is_empty() {
        return Err(ResponseError::bad_request("account_uids 不能为空"));
    }

    let normalized_uids = normalize_account_uids(account_uids);
    let found = load_accessible_requested_uids(pool, owner_username, &normalized_uids)
        .await
        .map_err(ResponseError::internal)?;
    for uid in normalized_uids {
        if !found.contains(&uid) {
            return Err(ResponseError::not_found(format!("账号 {uid} 不存在")));
        }
    }
    Ok(())
}

pub(crate) async fn ensure_account_uid_accessible(
    pool: &SqlitePool,
    owner_username: Option<&str>,
    account_uid: &str,
) -> Result<(), ResponseError> {
    ensure_account_uids_accessible(pool, owner_username, &[account_uid.trim().to_string()]).await
}

pub(crate) async fn account_uid_accessible(
    pool: &SqlitePool,
    owner_username: Option<&str>,
    account_uid: &str,
) -> Result<bool, sqlx::Error> {
    let uid = account_uid.trim();
    let mut uids = load_accessible_requested_uids(pool, owner_username, &[uid.to_string()]).await?;
    Ok(uids.remove(uid))
}

pub(crate) async fn active_account_uid_accessible(
    pool: &SqlitePool,
    owner_username: Option<&str>,
    account_uid: &str,
) -> Result<bool, sqlx::Error> {
    let uid = account_uid.trim();
    let owner = owner_username
        .map(str::trim)
        .filter(|owner| !owner.is_empty());
    let exists = match owner {
        Some(owner) => sqlx::query(
            "SELECT 1 FROM accounts WHERE uid = ? AND owner_username = ? AND status = 'active' LIMIT 1",
        )
        .bind(uid)
        .bind(owner)
        .fetch_optional(pool)
        .await?,
        None => sqlx::query("SELECT 1 FROM accounts WHERE uid = ? AND status = 'active' LIMIT 1")
            .bind(uid)
            .fetch_optional(pool)
            .await?,
    };
    Ok(exists.is_some())
}

async fn load_accessible_requested_uids(
    pool: &SqlitePool,
    owner_username: Option<&str>,
    account_uids: &[String],
) -> Result<HashSet<String>, sqlx::Error> {
    let mut query = QueryBuilder::<Sqlite>::new("SELECT uid FROM accounts WHERE uid IN (");
    let mut separated = query.separated(", ");
    for uid in account_uids {
        separated.push_bind(uid);
    }
    separated.push_unseparated(")");
    query.push(" AND status != 'disabled'");
    if let Some(owner) = owner_username {
        query.push(" AND owner_username = ");
        query.push_bind(owner);
    }

    let rows = query.build().fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("uid").ok())
        .collect())
}

fn normalize_account_uids(account_uids: &[String]) -> Vec<String> {
    account_uids
        .iter()
        .map(|uid| uid.trim().to_string())
        .collect()
}
