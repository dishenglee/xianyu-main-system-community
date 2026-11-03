use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::{Pool, Sqlite};

mod cookies;
mod http;
mod passport;
mod persistence;

// Re-export the one public function from persistence that external callers use directly.
pub use persistence::mark_business_session_failure;

// ---------- constants ----------

const APPKEY: &str = "34839810";
#[allow(non_snake_case)]
fn MTOP_BASE() -> &'static str {
    crate::sealed_protocol::sealed_str!("https://h5api.m.goofish.com/h5")
}
#[allow(non_snake_case)]
fn GOOFISH_URL() -> &'static str {
    crate::sealed_protocol::sealed_str!("https://www.goofish.com/")
}
#[allow(non_snake_case)]
fn SET_LOGIN_SETTINGS_URL() -> &'static str {
    crate::sealed_protocol::sealed_str!(
        "https://passport.goofish.com/ac/account/setLoginSettings.do"
    )
}
#[allow(non_snake_case)]
fn PASSPORT_MINI_LOGIN_URL() -> &'static str {
    crate::sealed_protocol::sealed_str!("https://passport.goofish.com/mini_login.htm?lang=zh_cn&appName=xianyu&appEntrance=web&styleType=vertical&notLoadSsoView=false&notKeepLogin=false&isMobile=false&qrCodeFirst=false&site=77")
}
fn passport_login_check_url() -> &'static str {
    crate::sealed_protocol::sealed_str!(
        "https://passport.goofish.com/login_check.htm?redirectURL=https%3A%2F%2Fwww.goofish.com%2F"
    )
}
fn passport_has_login_url() -> &'static str {
    crate::sealed_protocol::sealed_str!(
        "https://passport.goofish.com/newlogin/hasLogin.do?appName=xianyu&fromSite=77"
    )
}
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const HAVANA_RENEW_THRESHOLD_SECS: i64 = 3 * 86400;
const BLOCKED_COOKIE_PREFIXES: &[&str] = &["x5sec"];

// ---------- public types ----------

#[derive(Debug, Clone)]
pub struct RefreshOutcome {
    pub ok: bool,
    pub detail: String,
    pub reason: String,
    pub refreshed_at: f64,
    pub cookies: BTreeMap<String, String>,
}

impl RefreshOutcome {
    fn failure(
        detail: impl Into<String>,
        reason: impl Into<String>,
        cookies: BTreeMap<String, String>,
    ) -> Self {
        Self {
            ok: false,
            detail: detail.into(),
            reason: reason.into(),
            refreshed_at: 0.0,
            cookies,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BusinessSessionProbe {
    pub ok: bool,
    pub management_kind: crate::mtop_errors::MtopErrorKind,
    /// Steps tried in order (e.g. `["probe", "resume", "probe", "reanchor", "probe"]`),
    /// useful for logging why we ended up where we did.
    pub steps: Vec<String>,
    pub ret_code: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusinessTransport {
    H5,
    Wx,
    App,
}

// ---------- public API ----------

/// Orchestrate a full cookie refresh with retry + exponential backoff.
///
/// The inner `refresh_account_cookies_once` is idempotent (it always merges
/// server-returned cookies into whatever was persisted), so re-running on
/// transient passport/MTOP hiccups is safe. We only retry on `Err` or on a
/// soft-failure outcome whose reason suggests transient upstream trouble
/// (network timeout, passport 5xx, etc.); login-base-missing is terminal.
pub async fn refresh_account_cookies(
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<RefreshOutcome, String> {
    const MAX_ATTEMPTS: u32 = 3;
    let mut last_err: Option<String> = None;
    let mut last_soft: Option<RefreshOutcome> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        match refresh_account_cookies_once(accounts, uid).await {
            Ok(outcome) if outcome.ok => return Ok(outcome),
            Ok(outcome) => {
                // Terminal soft-failures should not be retried.
                if matches!(
                    outcome.reason.as_str(),
                    "login_base_missing" | "account_not_found"
                ) {
                    return Ok(outcome);
                }
                last_soft = Some(outcome);
            }
            Err(e) => last_err = Some(e),
        }
        if attempt < MAX_ATTEMPTS {
            // 500ms, 1s, 2s — capped, with a small additive offset derived
            // from the wallclock nanos to scatter concurrent refreshes.
            let base_ms = 500u64.saturating_mul(1u64 << (attempt - 1));
            let jitter = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| (d.subsec_nanos() as u64) % 200)
                .unwrap_or(0);
            tokio::time::sleep(Duration::from_millis(base_ms + jitter)).await;
        }
    }

    if let Some(soft) = last_soft {
        return Ok(soft);
    }
    Err(last_err.unwrap_or_else(|| "cookie refresh failed without detail".to_string()))
}

async fn refresh_account_cookies_once(
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<RefreshOutcome, String> {
    use cookies::{has_recoverable_login_base, merge_in, merged_cookie_map};
    use passport::{
        bootstrap_h5tk, enable_long_login, havana_needs_refresh, http_reanchor_session,
    };
    use persistence::{load_account, persist_failure, persist_success, unix_seconds_now};

    let account = load_account(accounts, uid).await?;
    let old_cookies = merged_cookie_map(&account.cookies, &account.browser_cookie_jar);

    if !has_recoverable_login_base(&old_cookies) {
        let outcome =
            RefreshOutcome::failure("登录态缺失", "login_base_missing", old_cookies.clone());
        persist_failure(accounts, uid, &outcome, /*clear_token=*/ true).await?;
        return Ok(outcome);
    }

    let bootstrap = bootstrap_h5tk(&old_cookies).await;
    let (mut refreshed, mut reason) = match bootstrap {
        Ok(cookies) if cookies.get("_m_h5_tk").is_some() => (cookies, "bootstrap_h5tk".to_string()),
        Ok(_) | Err(_) => {
            let reanchored = http_reanchor_session(&old_cookies)
                .await
                .unwrap_or_default();
            if reanchored.get("_m_h5_tk").is_some() {
                (reanchored, "http_reanchor_session".to_string())
            } else {
                let mut merged = old_cookies.clone();
                merge_in(&mut merged, reanchored);
                let outcome = RefreshOutcome::failure(
                    "bootstrap 未获取到 _m_h5_tk",
                    "bootstrap_no_h5tk",
                    merged,
                );
                persist_failure(accounts, uid, &outcome, false).await?;
                return Ok(outcome);
            }
        }
    };

    if havana_needs_refresh(&refreshed) {
        if let Ok(with_havana) = enable_long_login(&refreshed).await {
            refreshed = with_havana;
            if !reason.contains("havana") {
                reason = format!("{reason}+havana");
            }
        }
    }

    let mut merged = old_cookies;
    merge_in(&mut merged, refreshed);

    let now = unix_seconds_now();
    persist_success(accounts, uid, &merged, now, &reason).await?;

    Ok(RefreshOutcome {
        ok: true,
        detail: "bootstrap 刷新成功".to_string(),
        reason,
        refreshed_at: now,
        cookies: merged,
    })
}

/// Deep version of `refresh_account_cookies` — runs the shallow refresh first,
/// then additionally invokes `http_resume_haslogin_session` to renew the
/// long-lived login base (unb/sgcookie/cookie2). Used by the live-socket
/// reconnect loop when the shallow refresh keeps failing upstream auth.
pub async fn refresh_account_cookies_deep(
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<RefreshOutcome, String> {
    use cookies::{has_recoverable_login_base, merge_in};
    use passport::{bootstrap_h5tk, http_resume_haslogin_session};
    use persistence::{persist_success, unix_seconds_now};

    // 1. 先跑浅刷新（bootstrap_h5tk + 可能的 reanchor）。即使它 ok=true
    //    我们也还要继续跑 hasLogin 层 —— 浅层只能刷 h5_tk，长效登录基座
    //    必须靠 hasLogin 才能续期。
    let shallow = refresh_account_cookies(accounts, uid).await?;
    let base_cookies = shallow.cookies.clone();

    if !has_recoverable_login_base(&base_cookies) {
        // 没有 unb/sgcookie —— hasLogin 救不了。直接返回浅刷结果。
        return Ok(shallow);
    }

    // 2. hasLogin 续期 long-lived 登录基座（unb / sgcookie / cookie2 / _tb_token_）。
    let resumed = match http_resume_haslogin_session(&base_cookies).await {
        Ok(c) => c,
        Err(error) => {
            tracing::warn!(uid = %uid, error = %error, "http_resume_haslogin_session failed");
            return Ok(shallow); // 回退：浅刷结果已写入 DB。
        }
    };

    // 3. resume 后有新 cookies 才再补一次 bootstrap_h5tk（把 _m_h5_tk 和新的 unb 对齐）。
    let final_cookies = match bootstrap_h5tk(&resumed).await {
        Ok(post_bootstrap) if post_bootstrap.get("_m_h5_tk").is_some() => {
            let mut merged = resumed.clone();
            merge_in(&mut merged, post_bootstrap);
            merged
        }
        _ => resumed.clone(),
    };

    let now = unix_seconds_now();
    persist_success(accounts, uid, &final_cookies, now, "hasLogin_resume").await?;

    Ok(RefreshOutcome {
        ok: true,
        detail: "深度刷新：hasLogin 已续期登录基座".to_string(),
        reason: format!("{}+hasLogin_resume", shallow.reason),
        refreshed_at: now,
        cookies: final_cookies,
    })
}

// =============================================================================
// Business-session probe & repair (Python `_repair_business_session` parity)
// =============================================================================
//
// After the cookie jar is rewritten by bootstrap_h5tk / hasLogin / reanchor,
// the only *real* test of whether we're back online is hitting a business
// MTOP endpoint and checking its `ret`. Python calls
// `mtop.taobao.idle.item.management.query` — a cheap "list my 1 item" probe.
//
// If the probe returns:
//   * SUCCESS  → business session alive, we're done.
//   * SESSION_EXPIRED / USER_NOT_LOGIN / NOT_LOGIN  → the deep refresh didn't
//     actually restore the login base (typical: `_tb_token_` / `cookie2` still
//     stale). Re-run hasLogin + reanchor, then re-probe once.
//   * anything else  → propagate the `MtopErrorKind` to the caller so risk/
//     schema/sign anomalies get alerted rather than silently retried.

/// Probe the business session with a single `item.management.query` call;
/// on session-expired, run the minimal repair ladder and probe again.
///
/// This mirrors `cookie_service._repair_business_session` — the probe is the
/// decisive "cookies really work" check that the purely token-based refresh
/// chain can't give us on its own.
pub async fn probe_and_repair_business_session(
    config: &crate::config::AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<BusinessSessionProbe, String> {
    use crate::mtop_errors::{classify_mtop_payload, MtopErrorKind};
    use cookies::{has_recoverable_login_base, merge_in};
    use passport::{bootstrap_h5tk, http_reanchor_session, http_resume_haslogin_session};
    use persistence::{load_cookies_map, persist_success, unix_seconds_now};
    use serde_json::Value;

    let mut steps: Vec<String> = Vec::new();
    let mut last_kind = MtopErrorKind::Unknown(String::new());
    let mut last_code = String::new();

    // Attempt ladder: (label, optional repair before probe)
    //   1. initial probe (no repair)
    //   2. hasLogin resume, then probe
    //   3. reanchor, then probe
    //
    // We don't retry forever: 3 probes is enough to tell us whether the
    // session is structurally broken (login base gone) vs a transient hiccup.
    enum Step {
        InitialProbe,
        ResumeThenProbe,
        ReanchorThenProbe,
    }
    let plan = [
        Step::InitialProbe,
        Step::ResumeThenProbe,
        Step::ReanchorThenProbe,
    ];

    for step in plan.iter() {
        match step {
            Step::InitialProbe => {
                steps.push("probe".to_string());
            }
            Step::ResumeThenProbe => {
                // Only attempt if the login base is still recoverable;
                // otherwise hasLogin will just burn requests.
                let base = match load_cookies_map(accounts, uid).await {
                    Ok(c) => c,
                    Err(error) => return Err(error),
                };
                if !has_recoverable_login_base(&base) {
                    steps.push("skip_resume_no_base".to_string());
                    break;
                }
                steps.push("resume".to_string());
                match http_resume_haslogin_session(&base).await {
                    Ok(resumed) => {
                        // Realign _m_h5_tk with the fresh unb.
                        let merged = match bootstrap_h5tk(&resumed).await {
                            Ok(b) if b.get("_m_h5_tk").is_some() => {
                                let mut m = resumed.clone();
                                merge_in(&mut m, b);
                                m
                            }
                            _ => resumed,
                        };
                        let now = unix_seconds_now();
                        persist_success(accounts, uid, &merged, now, "probe_resume").await?;
                    }
                    Err(error) => {
                        tracing::warn!(
                            uid = %uid, error = %error,
                            "probe: hasLogin resume failed, trying reanchor next"
                        );
                    }
                }
                steps.push("probe".to_string());
            }
            Step::ReanchorThenProbe => {
                let base = load_cookies_map(accounts, uid).await?;
                steps.push("reanchor".to_string());
                match http_reanchor_session(&base).await {
                    Ok(reanchored) => {
                        let now = unix_seconds_now();
                        persist_success(accounts, uid, &reanchored, now, "probe_reanchor").await?;
                    }
                    Err(error) => {
                        tracing::warn!(
                            uid = %uid, error = %error,
                            "probe: reanchor failed"
                        );
                    }
                }
                steps.push("probe".to_string());
            }
        }

        // Run the probe. `h5_request` internally reads fresh cookies from
        // the accounts DB on every call, so the pre-step persist above is
        // what actually makes the retry see new auth.
        let payload = serde_json::json!({
            "auctionTypes": ["b", "d", "a"],
            "newVersion": "2.0",
            "onlineSubTab": 1,
            "pageNumber": 1,
            "rowsPerPage": 1,
        });
        let protocol = crate::protocol_catalog::resolve_builtin_protocol(
            crate::protocol_catalog::opcodes::ACCOUNT_ITEM_MANAGEMENT_QUERY,
        )
        .map_err(|error| error.to_string())?;
        let upstream = crate::xianyu_client::h5_request_no_session_repair(
            config,
            accounts,
            uid,
            protocol.api(),
            protocol.version(),
            &payload,
            Vec::new(),
        )
        .await;

        match upstream {
            Ok(body) => {
                last_kind = classify_mtop_payload(&body);
                last_code = body
                    .get("ret")
                    .and_then(Value::as_array)
                    .and_then(|arr| arr.first())
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if matches!(last_kind, MtopErrorKind::Ok) {
                    return Ok(BusinessSessionProbe {
                        ok: true,
                        management_kind: last_kind,
                        steps,
                        ret_code: last_code,
                    });
                } else if !matches!(last_kind, MtopErrorKind::SessionExpired) {
                    return Ok(BusinessSessionProbe {
                        ok: false,
                        management_kind: last_kind,
                        steps,
                        ret_code: last_code,
                    });
                }
            }
            Err(error) => {
                // Network / transport failure — record and try the next step.
                tracing::warn!(uid = %uid, error = %error, "probe MTOP transport failed");
                last_kind = MtopErrorKind::Unknown(error.clone());
                last_code = error;
            }
        }
    }

    Ok(BusinessSessionProbe {
        ok: false,
        management_kind: last_kind,
        steps,
        ret_code: last_code,
    })
}

pub async fn repair_business_session_for_transport(
    config: &crate::config::AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    transport: BusinessTransport,
) -> Result<BusinessSessionProbe, String> {
    match transport {
        BusinessTransport::H5 | BusinessTransport::Wx => {
            probe_and_repair_business_session(config, accounts, uid).await
        }
        BusinessTransport::App => {
            let refresh = refresh_account_cookies_deep(accounts, uid).await?;
            let ret_code = if refresh.ok {
                "deep_refresh_ok".to_string()
            } else if refresh.detail.trim().is_empty() {
                refresh.reason.clone()
            } else {
                refresh.detail.clone()
            };
            Ok(BusinessSessionProbe {
                ok: refresh.ok,
                management_kind: if refresh.ok {
                    crate::mtop_errors::MtopErrorKind::Ok
                } else {
                    crate::mtop_errors::MtopErrorKind::SessionExpired
                },
                steps: vec!["deep_refresh".to_string()],
                ret_code,
            })
        }
    }
}

pub async fn try_repair_business_session(
    config: &crate::config::AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    transport: BusinessTransport,
) -> bool {
    match repair_business_session_for_transport(config, accounts, uid, transport).await {
        Ok(probe) if probe.ok => {
            tracing::info!(
                uid = %uid,
                api = %api,
                transport = ?transport,
                steps = ?probe.steps,
                ret = %probe.ret_code,
                "session repaired before retry"
            );
            true
        }
        Ok(probe) => {
            tracing::warn!(
                uid = %uid,
                api = %api,
                transport = ?transport,
                steps = ?probe.steps,
                kind = %probe.management_kind.tag(),
                ret = %probe.ret_code,
                "session repair probe finished without recovery"
            );
            let _ = mark_business_session_failure(
                accounts,
                uid,
                &format!("business_probe_failed: {}", probe.ret_code),
            )
            .await;
            false
        }
        Err(error) => {
            tracing::warn!(
                uid = %uid,
                api = %api,
                transport = ?transport,
                error = %error,
                "session repair errored"
            );
            let _ = mark_business_session_failure(
                accounts,
                uid,
                &format!("business_probe_error: {error}"),
            )
            .await;
            false
        }
    }
}
