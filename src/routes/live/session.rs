use std::{collections::HashSet, time::Duration};

use sqlx::Row;
use tokio::{sync::watch, task::JoinHandle};

use crate::{
    routes::accounts::runtime_access::{
        ensure_account_can_call_upstream, resolve_accounts_runtime_access,
    },
    state::AppState,
};

pub(super) struct LiveAccountTask {
    pub(super) shutdown_tx: watch::Sender<bool>,
    pub(super) handle: JoinHandle<()>,
}

pub(super) async fn reconcile_live_account_tasks(
    state: &AppState,
    tasks: &mut std::collections::HashMap<String, LiveAccountTask>,
) -> Result<usize, String> {
    let finished = tasks
        .iter()
        .filter_map(|(uid, task)| task.handle.is_finished().then_some(uid.clone()))
        .collect::<Vec<_>>();
    for uid in finished {
        if let Some(task) = tasks.remove(&uid) {
            let _ = task.handle.await;
        }
    }

    let active_uids = list_active_account_uids(state.storage().accounts())
        .await
        .map_err(|error| error.to_string())?;
    let runtime_access_by_uid = resolve_accounts_runtime_access(state, &active_uids)
        .await
        .map_err(|error| error.detail().to_string())?;
    let mut allowed_uids = Vec::with_capacity(active_uids.len());
    for uid in active_uids {
        if let Some(access) = runtime_access_by_uid.get(&uid) {
            if access.delete_only {
                publish_connection_state(
                    state,
                    &uid,
                    "restricted",
                    0,
                    &access.remote_binding_message,
                )
                .await;
                continue;
            }
        }
        allowed_uids.push(uid);
    }
    let active_set = allowed_uids.iter().cloned().collect::<HashSet<_>>();

    let stale = tasks
        .keys()
        .filter(|uid| !active_set.contains(uid.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for uid in stale {
        if let Some(task) = tasks.remove(&uid) {
            let _ = task.shutdown_tx.send(true);
            let _ = task.handle.await;
        }
    }

    // Spawn workers with a staggered startup delay. The ACCS gateway
    // throttles burst connections from a single IP — when 26 workers all
    // open WSs in the same second the server RSTs most of them within ~8s
    // (we counted >120 resets in 90s). 1.5s spacing keeps the 26-account
    // ramp under 40s and dramatically improves first-connect success rate.
    let mut stagger_index = 0_u64;
    for uid in &allowed_uids {
        if tasks.contains_key(uid) {
            continue;
        }
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let state_clone = state.clone();
        let uid_clone = uid.clone();
        let delay = Duration::from_millis(stagger_index.saturating_mul(1500));
        stagger_index += 1;
        let handle = tokio::spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            super::worker::run_live_account_worker(state_clone, uid_clone, shutdown_rx).await;
        });
        tasks.insert(
            uid.clone(),
            LiveAccountTask {
                shutdown_tx,
                handle,
            },
        );
    }

    Ok(allowed_uids.len())
}

pub(super) async fn shutdown_live_account_tasks(
    tasks: &mut std::collections::HashMap<String, LiveAccountTask>,
) {
    let drained = tasks.drain().collect::<Vec<_>>();
    for (_, task) in &drained {
        let _ = task.shutdown_tx.send(true);
    }
    for (_, task) in drained {
        let _ = task.handle.await;
    }
}

pub async fn list_active_account_uids(
    pool: &sqlx::Pool<sqlx::Sqlite>,
) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT uid FROM accounts WHERE status = 'active' ORDER BY updated_at DESC, uid ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| row.get::<String, _>("uid"))
        .filter(|uid| !uid.trim().is_empty())
        .collect())
}

pub(super) async fn publish_connection_state(
    state: &AppState,
    uid: &str,
    status: &str,
    reconnect_attempts: i64,
    reason: &str,
) {
    let _ = state
        .storage()
        .upsert_live_connection_state(uid, status, reconnect_attempts, reason)
        .await;
    state.live_events().publish(serde_json::json!({
        "type": format!("connection.{status}"),
        "uid": uid,
        "reason": reason,
        "channel": "wx",
    }));
}

pub(super) fn reconnect_delay(attempt: i64) -> Duration {
    // Python parity: `xianyu_wx_live_client.py:289-290`
    //   min(300.0, float(2 ** min(self._reconnect_attempts, 8)))
    // attempts=0 → 1s, =1 → 2s, =2 → 4s, ..., =8 → 256s, =9+ → 300s (capped).
    let capped = attempt.clamp(0, 8) as u32;
    let seconds = 2_u64
        .saturating_pow(capped)
        .min(super::LIVE_RECONNECT_MAX.as_secs());
    Duration::from_secs(seconds)
}

/// Python parity: distinguish "session is dead, must deep-refresh cookies"
/// from "_m_h5_tk just expired, fetch_wx_live_access_token's round-2 already
/// handled it". Only the former warrants a `hasLogin.do` deep refresh.
///
/// Why split: the previous Rust impl matched any error containing "TOKEN" /
/// "SESSION" / "401" / "403" / "ILLEGAL_ACCESS" and triggered
/// `refresh_account_cookies_deep` for all of them. Deep refresh re-runs the
/// passport handshake (hasLogin.do, havana, enable_long_login), which from
/// goofish's anti-fraud view looks like a re-login. Doing this on every
/// `_m_h5_tk` rotation (which happens every ~6 hours naturally) is a strong
/// "this is a bot" signal and accelerates account quarantine.
///
/// Python's `_fetch_access_token` (`xianyu_wx_live_client.py:317-329`)
/// transparently handles `_m_h5_tk` expiry via the round-2 new-c retry
/// inside the same function — the worker loop never sees that error. Only
/// when the long-lived login base (`unb` / `sgcookie` / `cookie2`) is gone
/// does the worker get a real error, and that's when deep refresh is
/// appropriate. We mirror that boundary here.
pub(super) fn is_session_dead_error(error: &str) -> bool {
    let upper = error.to_ascii_uppercase();
    [
        "SESSION_EXPIRED",
        "USER_NOT_LOGIN",
        "USER_LOGIN_AUTH_EXPIRED",
        "NOT_LOGIN",
        "NOT LOGIN",
        "ILLEGAL_ACCESS",
        "FAIL_SYS_AUTH_NOT_LOGIN",
        "FAIL_SYS_LOGIN_REQUIRED",
    ]
    .iter()
    .any(|needle| upper.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn active_account_discovery_is_not_scoped_to_license_owner() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE accounts (uid TEXT PRIMARY KEY, status TEXT NOT NULL, owner_username TEXT, updated_at REAL NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO accounts (uid, status, owner_username, updated_at) VALUES ('local', 'active', '', 2), ('another-owner', 'active', 'someone-else', 1), ('disabled', 'disabled', '', 3)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let active = list_active_account_uids(&pool).await.unwrap();

        assert_eq!(active, vec!["local", "another-owner"]);
    }
}

/// Refresh cookies for `uid` and update the account's persisted status based
/// on the outcome. Returns true if refresh succeeded, false otherwise.
///
/// Uses the **deep** refresh path (`refresh_account_cookies_deep`) so that
/// `unb / sgcookie / cookie2 / _tb_token_` get renewed via the passport
/// `hasLogin.do` handshake, not only `_m_h5_tk`. Without the deep layer the
/// live socket keeps hitting `FAIL_SYS_TOKEN_ILLEGAL` even after shallow
/// refreshes because the long-lived login base has drifted.
pub(super) async fn refresh_cookies_for_handshake(state: &AppState, uid: &str) -> bool {
    if let Err(error) = ensure_account_can_call_upstream(state, None, uid).await {
        tracing::warn!(
            uid = %uid,
            error = %error,
            "skip live handshake refresh because remote binding is delete-only"
        );
        return false;
    }
    match crate::cookie_refresh::refresh_account_cookies_deep(state.storage().accounts(), uid).await
    {
        Ok(outcome) if outcome.ok => {
            tracing::info!(
                uid = %uid,
                reason = %outcome.reason,
                "cookie refresh succeeded after ACCS handshake failure"
            );
            // Python parity: verify with a real business MTOP before trusting
            // the refresh. If management.query still says SESSION_EXPIRED, the
            // probe runs its own minimal repair ladder (resume → reanchor) and
            // re-probes. Non-session errors (sign/schema/risk) fall through —
            // they aren't auth issues and shouldn't block the handshake.
            let accounts = state.storage().accounts();
            match crate::cookie_refresh::probe_and_repair_business_session(
                state.config(),
                accounts,
                uid,
            )
            .await
            {
                Ok(probe) if probe.ok => {
                    tracing::info!(
                        uid = %uid,
                        steps = ?probe.steps,
                        ret = %probe.ret_code,
                        "business session probe ok"
                    );
                }
                Ok(probe) => {
                    tracing::warn!(
                        uid = %uid,
                        steps = ?probe.steps,
                        kind = %probe.management_kind.tag(),
                        ret = %probe.ret_code,
                        "business session probe failed after repair ladder"
                    );
                    let _ = crate::cookie_refresh::mark_business_session_failure(
                        accounts,
                        uid,
                        &format!("live_probe_failed: {}", probe.ret_code),
                    )
                    .await;
                }
                Err(error) => {
                    tracing::warn!(
                        uid = %uid, error = %error,
                        "business session probe errored (transport)"
                    );
                    let _ = crate::cookie_refresh::mark_business_session_failure(
                        accounts,
                        uid,
                        &format!("live_probe_error: {error}"),
                    )
                    .await;
                }
            }
            true
        }
        Ok(outcome) => {
            tracing::warn!(
                uid = %uid,
                detail = %outcome.detail,
                reason = %outcome.reason,
                "cookie refresh failed — account likely needs re-login"
            );
            false
        }
        Err(error) => {
            tracing::warn!(
                uid = %uid,
                error = %error,
                "cookie refresh errored during ACCS handshake retry"
            );
            false
        }
    }
}
