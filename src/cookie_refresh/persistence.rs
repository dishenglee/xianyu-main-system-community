use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{Pool, Row, Sqlite};

use super::RefreshOutcome;

pub(super) struct AccountRow {
    pub(super) cookies: String,
    pub(super) browser_cookie_jar: String,
}

pub(super) async fn load_account(accounts: &Pool<Sqlite>, uid: &str) -> Result<AccountRow, String> {
    let row = sqlx::query("SELECT cookies, browser_cookie_jar FROM accounts WHERE uid = ? LIMIT 1")
        .bind(uid)
        .fetch_optional(accounts)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("account {uid} not found"))?;
    Ok(AccountRow {
        cookies: row.get::<String, _>("cookies"),
        browser_cookie_jar: row.get::<String, _>("browser_cookie_jar"),
    })
}

pub(super) async fn load_cookies_map(
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<BTreeMap<String, String>, String> {
    let row = load_account(accounts, uid).await?;
    Ok(super::cookies::merged_cookie_map(
        &row.cookies,
        &row.browser_cookie_jar,
    ))
}

pub(super) async fn persist_success(
    accounts: &Pool<Sqlite>,
    uid: &str,
    cookies: &BTreeMap<String, String>,
    now: f64,
    reason: &str,
) -> Result<(), String> {
    let cookies_json = serde_json::to_string(cookies).map_err(|error| error.to_string())?;
    let token = cookies.get("_m_h5_tk").cloned().unwrap_or_default();
    sqlx::query(
        r#"
        UPDATE accounts
        SET token = ?,
            cookies = ?,
            cookie_refreshed_at = ?,
            last_session_check_at = ?,
            last_session_ok_at = ?,
            first_session_fail_at = 0,
            last_session_status = 'ok',
            last_session_reason = ?,
            status = 'active',
            updated_at = ?
        WHERE uid = ?
        "#,
    )
    .bind(token)
    .bind(cookies_json)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(reason)
    .bind(now)
    .bind(uid)
    .execute(accounts)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) async fn persist_failure(
    accounts: &Pool<Sqlite>,
    uid: &str,
    outcome: &RefreshOutcome,
    clear_token: bool,
) -> Result<(), String> {
    let now = unix_seconds_now();
    let status = if clear_token {
        "expired"
    } else {
        "soft_failed"
    };
    let token_update = if clear_token { "token = ''," } else { "" };
    let query = format!(
        r#"
        UPDATE accounts
        SET {token_update}
            last_session_check_at = ?,
            last_session_status = 'error',
            last_session_reason = ?,
            status = CASE WHEN ? = 'expired' THEN 'expired' ELSE status END,
            updated_at = ?
        WHERE uid = ?
        "#
    );
    sqlx::query(&query)
        .bind(now)
        .bind(&outcome.reason)
        .bind(status)
        .bind(now)
        .bind(uid)
        .execute(accounts)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn should_mark_account_expired(reason: &str) -> bool {
    let upper = reason.to_ascii_uppercase();
    [
        "SESSION_EXPIRED",
        "USER_NOT_LOGIN",
        "NOT_LOGIN",
        "USER_LOGIN_AUTH_EXPIRED",
        "TOKEN_EMPTY",
        "TOKEN_EXOIRED",
        "TOKEN_EXPIRED",
        "TOKEN_ILLEGAL",
    ]
    .iter()
    .any(|needle| upper.contains(needle))
}

pub async fn mark_business_session_failure(
    accounts: &Pool<Sqlite>,
    uid: &str,
    reason: &str,
) -> Result<(), String> {
    let now = unix_seconds_now();
    let mark_expired = if should_mark_account_expired(reason) {
        1_i64
    } else {
        0_i64
    };
    sqlx::query(
        r#"
        UPDATE accounts
        SET last_session_check_at = ?,
            last_session_status = 'error',
            last_session_reason = ?,
            status = CASE WHEN ? = 1 THEN 'expired' ELSE status END,
            updated_at = ?
        WHERE uid = ?
        "#,
    )
    .bind(now)
    .bind(reason)
    .bind(mark_expired)
    .bind(now)
    .bind(uid)
    .execute(accounts)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn unix_seconds_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::should_mark_account_expired;

    #[test]
    fn session_expired_reason_marks_account_expired() {
        assert!(should_mark_account_expired(
            "business_probe_failed: FAIL_SYS_SESSION_EXPIRED::Session过期"
        ));
        assert!(should_mark_account_expired(
            "live_probe_failed: FAIL_SYS_USER_NOT_LOGIN::未登录"
        ));
    }

    #[test]
    fn non_auth_reason_does_not_mark_account_expired() {
        assert!(!should_mark_account_expired(
            "business_probe_failed: FAIL_BIZ_ITEM_OFF_SHELF::宝贝已下架"
        ));
    }
}
