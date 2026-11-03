//! MTOP response classification.
//!
//! Every MTOP response carries a `ret` array of `"CODE::人类可读描述"` strings.
//! The first entry is the primary status. This module turns that primary code
//! into a `MtopErrorKind` so the caller can decide:
//!
//!   * `Ok`           — success, unwrap `data` and go.
//!   * `TokenExpired` — refresh `_m_h5_tk` and retry exactly once.
//!   * `SessionExpired` — cookie-level auth gone, trigger full cookie refresh.
//!   * `SignInvalid`  — sign/appKey mismatch; **alert**, likely upstream change.
//!   * `SchemaChanged` — required field missing/invalid; **alert**, likely
//!                       upstream API signature change.
//!   * `RiskControl`  — throttled / sliding-captcha territory; back off + maybe
//!                      rotate proxy.
//!   * `BizError`     — business-level failure (stock empty, item removed, etc).
//!   * `Unknown`      — an unrecognised code; **alert** so the classifier can
//!                      be extended.
//!
//! The classifier is deliberately conservative: when in doubt, map to
//! `Unknown` rather than silently absorbing into `BizError`. An alert on a
//! classification gap is much cheaper than mysterious account breakage.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MtopErrorKind {
    Ok,
    TokenExpired,
    SessionExpired,
    SignInvalid,
    SchemaChanged,
    RiskControl,
    BizError(String),
    Unknown(String),
}

impl MtopErrorKind {
    /// Short tag for metrics/logs. Always a stable, lowercase identifier.
    pub fn tag(&self) -> &'static str {
        match self {
            MtopErrorKind::Ok => "ok",
            MtopErrorKind::TokenExpired => "token_expired",
            MtopErrorKind::SessionExpired => "session_expired",
            MtopErrorKind::SignInvalid => "sign_invalid",
            MtopErrorKind::SchemaChanged => "schema_changed",
            MtopErrorKind::RiskControl => "risk_control",
            MtopErrorKind::BizError(_) => "biz_error",
            MtopErrorKind::Unknown(_) => "unknown",
        }
    }

    /// Is this an "upstream probably changed" anomaly worth alerting on?
    pub fn is_anomaly(&self) -> bool {
        matches!(
            self,
            MtopErrorKind::SignInvalid | MtopErrorKind::SchemaChanged | MtopErrorKind::Unknown(_)
        )
    }

    pub fn is_success(&self) -> bool {
        matches!(self, MtopErrorKind::Ok)
    }

    pub fn is_auth_recoverable(&self) -> bool {
        matches!(
            self,
            MtopErrorKind::TokenExpired | MtopErrorKind::SessionExpired
        )
    }
}

/// Classify an MTOP response body. Walks `payload.ret[0]` and delegates to
/// [`classify_ret_code`].
pub fn classify_mtop_payload(payload: &Value) -> MtopErrorKind {
    let code = payload
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str)
        .unwrap_or("");
    classify_ret_code(code)
}

/// Classify a single MTOP ret code string, e.g. `"FAIL_SYS_TOKEN_EXOIRED::令牌过期"`.
///
/// Order of checks matters: the more specific patterns come first so that
/// something like `FAIL_SYS_TOKEN_FAIL` maps to `TokenExpired` rather than
/// landing in the generic `FAIL_SYS_*` bucket.
pub fn classify_ret_code(raw: &str) -> MtopErrorKind {
    let code = raw.split("::").next().unwrap_or(raw).trim();
    if code.is_empty() {
        return MtopErrorKind::Unknown(raw.to_string());
    }
    let upper = code.to_ascii_uppercase();

    if upper.starts_with("SUCCESS") || upper == "OK" {
        return MtopErrorKind::Ok;
    }

    // Token-level: short-lived _m_h5_tk is bad, resign will fix it.
    //
    // Note: Taobao's actual code is `FAIL_SYS_TOKEN_EXOIRED` (sic, typo'd on
    // their side — do not "correct" to EXPIRED or we'll miss it).
    //
    // IMPORTANT: `FAIL_SYS_TOKEN_ILLEGAL::非法令牌` is *also* recoverable —
    // the server hands back a fresh `c` (new `_m_h5_tk;_m_h5_tk_enc`) in the
    // response and a resign with that new c succeeds. Python's live client
    // classifies it the same way (`"TOKEN" in ret.upper()`). Before this fix,
    // Rust was treating TOKEN_ILLEGAL as `Unknown` and bailing out, which is
    // why every wx.login.token call was dying for web-QR-logged accounts.
    if upper.contains("TOKEN_EMPTY")
        || upper.contains("TOKEN_EXOIRED")
        || upper.contains("TOKEN_EXPIRED")
        || upper.contains("TOKEN_ILLEGAL")
        || upper == "FAIL_SYS_TOKEN_FAIL"
    {
        return MtopErrorKind::TokenExpired;
    }

    // Session-level: cookie login state is gone, full refresh required.
    if upper.contains("SESSION_EXPIRED")
        || upper.contains("USER_NOT_LOGIN")
        || upper.contains("NOT_LOGIN")
        || upper.contains("USER_LOGIN_AUTH_EXPIRED")
    {
        return MtopErrorKind::SessionExpired;
    }

    // Sign / appKey — the signature machinery itself is broken.
    if upper.contains("ILLEGAL_ACCESS")
        || upper.contains("ILLEGAL_REQUEST")
        || upper.contains("SIGN_ERROR")
        || upper.contains("SIGN_INVALID")
        || upper.contains("APPKEY_ILLEGAL")
        || upper.contains("FAIL_SYS_ILLEGAL_TOKEN")
    {
        return MtopErrorKind::SignInvalid;
    }

    // Schema — required field shape changed upstream.
    if upper.contains("PARAM_INVALID")
        || upper.contains("PARAM_MISSING")
        || upper.contains("PARAM_ERROR")
        || upper.contains("FAIL_SYS_PARAM")
    {
        return MtopErrorKind::SchemaChanged;
    }

    // Risk / throttle — slow down, rotate proxy, cool off.
    if upper.contains("TRAFFIC_LIMIT")
        || upper.contains("FLOWLIMIT")
        || upper.contains("SG_RISK")
        || upper.contains("RGV587")
        || upper.contains("RISK_CONTROL")
        || upper.contains("SM_CODE")
    {
        return MtopErrorKind::RiskControl;
    }

    // Business-level failures — normal domain errors, propagate verbatim.
    if upper.starts_with("FAIL_BIZ_") {
        return MtopErrorKind::BizError(raw.to_string());
    }

    // Generic system failures — often transient but we can't tell without
    // more info, so flag as unknown for visibility.
    MtopErrorKind::Unknown(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn success_codes_are_ok() {
        assert_eq!(classify_ret_code("SUCCESS::调用成功"), MtopErrorKind::Ok);
        assert_eq!(classify_ret_code("OK"), MtopErrorKind::Ok);
    }

    #[test]
    fn token_codes_hit_token_expired() {
        // The real typo'd Taobao code.
        assert_eq!(
            classify_ret_code("FAIL_SYS_TOKEN_EXOIRED::令牌过期"),
            MtopErrorKind::TokenExpired
        );
        assert_eq!(
            classify_ret_code("FAIL_SYS_TOKEN_EMPTY::令牌为空"),
            MtopErrorKind::TokenExpired
        );
        assert_eq!(
            classify_ret_code("FAIL_SYS_TOKEN_FAIL"),
            MtopErrorKind::TokenExpired
        );
    }

    #[test]
    fn session_codes_hit_session_expired() {
        assert_eq!(
            classify_ret_code("FAIL_SYS_SESSION_EXPIRED::会话过期"),
            MtopErrorKind::SessionExpired
        );
        assert_eq!(
            classify_ret_code("FAIL_SYS_USER_NOT_LOGIN::未登录"),
            MtopErrorKind::SessionExpired
        );
    }

    #[test]
    fn illegal_access_is_sign_invalid() {
        assert_eq!(
            classify_ret_code("FAIL_SYS_ILLEGAL_ACCESS::非法访问"),
            MtopErrorKind::SignInvalid
        );
        assert_eq!(
            classify_ret_code("ILLEGAL_ACCESS"),
            MtopErrorKind::SignInvalid
        );
    }

    #[test]
    fn param_errors_are_schema_changed() {
        assert_eq!(
            classify_ret_code("FAIL_SYS_PARAM_INVALID::参数非法"),
            MtopErrorKind::SchemaChanged
        );
    }

    #[test]
    fn risk_codes_hit_risk_control() {
        assert_eq!(
            classify_ret_code("FAIL_SYS_TRAFFIC_LIMIT::限流"),
            MtopErrorKind::RiskControl
        );
        assert_eq!(
            classify_ret_code("SG_RISK_VALIDATE"),
            MtopErrorKind::RiskControl
        );
    }

    #[test]
    fn biz_codes_pass_through() {
        let kind = classify_ret_code("FAIL_BIZ_ITEM_OFF_SHELF::宝贝已下架");
        match kind {
            MtopErrorKind::BizError(raw) => assert!(raw.contains("FAIL_BIZ_ITEM_OFF_SHELF")),
            other => panic!("expected BizError, got {other:?}"),
        }
    }

    #[test]
    fn unknown_code_surfaces_raw() {
        match classify_ret_code("FAIL_SYS_BRAND_NEW_ERROR::从未见过") {
            MtopErrorKind::Unknown(raw) => assert!(raw.contains("FAIL_SYS_BRAND_NEW_ERROR")),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn empty_ret_is_unknown() {
        assert!(matches!(classify_ret_code(""), MtopErrorKind::Unknown(_)));
    }

    #[test]
    fn classify_from_payload_uses_first_ret() {
        let payload = json!({"ret": ["SUCCESS::ok"], "data": {}});
        assert_eq!(classify_mtop_payload(&payload), MtopErrorKind::Ok);

        let payload = json!({"ret": ["FAIL_SYS_TOKEN_EXOIRED::过期"]});
        assert_eq!(classify_mtop_payload(&payload), MtopErrorKind::TokenExpired);
    }

    #[test]
    fn is_anomaly_flags_only_structural_issues() {
        assert!(MtopErrorKind::SignInvalid.is_anomaly());
        assert!(MtopErrorKind::SchemaChanged.is_anomaly());
        assert!(MtopErrorKind::Unknown("x".into()).is_anomaly());
        assert!(!MtopErrorKind::TokenExpired.is_anomaly());
        assert!(!MtopErrorKind::RiskControl.is_anomaly());
        assert!(!MtopErrorKind::BizError("x".into()).is_anomaly());
    }
}
