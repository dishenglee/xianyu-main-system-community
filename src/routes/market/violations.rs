//! Violation detection + dispatch helpers used by `items_sync.rs`.
//!
//! "Violation" here = upstream has put an item into some kind of penalty
//! state (审核未通过 / 违规下架 / 投诉处理 / 处罚封禁 / 涉嫌违规 / 限制…). The
//! signal is fuzzy — Xianyu doesn't expose a single canonical "isViolation"
//! flag. We instead walk a small set of fields on the upstream `cardData`
//! object and match against a keyword set. To avoid false positives on the
//! common "已售/主动下架" case, `canPolish == false` *alone* is NOT enough —
//! we require at least one explicit Chinese-text keyword hit in
//! `bizMessage` / `banner.title` / `headTitle` / `auditStatus` / etc., OR an
//! explicit status code like `REJECTED` / `BANNED`.
//!
//! When `items_sync` gets a hit, it calls `record_violation`. The storage
//! layer dedupes by (uid, item_id, violation_kind), so re-syncing an item
//! that's still in the same violation state is a no-op. Only fresh inserts
//! get a WeChat push.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use crate::{notifications::channel_send, state::AppState};

/// Keyword set checked against any string-valued field on the upstream card.
/// Order doesn't matter — first match wins, but we keep the kind buckets
/// distinct (`audit_failed`, `complaint`, `banned`, `other`) by inspecting
/// which keyword fired.
///
/// IMPORTANT: keep this list narrow — anything too generic ("下架" by
/// itself) collides with "主动下架/已售下架" which are NOT violations. Only
/// add a term here after confirming it's specifically a penalty / breach
/// signal in Xianyu's UI.
const VIOLATION_KEYWORDS: &[&str] = &[
    "违规",
    "违规下架",
    "违规商品",
    "违规商品下架",
    "审核未通过",
    "审核不通过",
    "审核失败",
    "涉嫌",
    "涉嫌违规",
    "投诉处理",
    "投诉",
    "已被处罚",
    "处罚",
    "封禁",
    "强制下架",
    "限制",
    "限流",
    "禁售",
];

/// Item status codes that unambiguously mean "violation" without needing a
/// keyword match. Compared case-insensitively against
/// `cardData.itemStatus` / `cardData.status`.
const VIOLATION_STATUS_CODES: &[&str] = &[
    "REJECTED",
    "BANNED",
    "AUDIT_FAILED",
    "AUDIT_REJECTED",
    "ILLEGAL",
    "PUNISHED",
];

/// A detection match.
#[derive(Debug, Clone)]
pub(super) struct DetectedViolation {
    /// One of "banned" | "audit_failed" | "complaint" | "other".
    pub kind: String,
    /// Human-readable Chinese reason — pulled from the first non-empty
    /// upstream string we matched against.
    pub reason: String,
    /// Compact JSON of which upstream fields fired the detection. Stored
    /// verbatim in `account_violations.raw_metadata` for debugging.
    pub raw_metadata: String,
}

/// Examine a single upstream card and return a `DetectedViolation` iff the
/// card looks like a violation. Conservative — see module doc.
pub(super) fn detect_violation(card: &Value, _item_id: &str) -> Option<DetectedViolation> {
    // Candidate string fields. Order = priority for which one gets surfaced
    // as `reason_text` (we take the FIRST non-empty match).
    let candidates: [(&str, Option<String>); 9] = [
        ("bizMessage", string_at(card, &["bizMessage"])),
        ("banner.title", banner_title(card)),
        ("headTitle", string_at(card, &["headTitle"])),
        ("auditStatus", string_at(card, &["auditStatus"])),
        ("auditStatusText", string_at(card, &["auditStatusText"])),
        ("subTitle", string_at(card, &["subTitle"])),
        ("statusText", string_at(card, &["statusText"])),
        ("bizCode", string_at(card, &["bizCode"])),
        ("offReason", string_at(card, &["offReason"])),
    ];

    let item_status = string_at(card, &["itemStatus"])
        .or_else(|| string_at(card, &["status"]))
        .unwrap_or_default();
    let item_status_upper = item_status.trim().to_ascii_uppercase();

    let status_hit = !item_status_upper.is_empty()
        && VIOLATION_STATUS_CODES
            .iter()
            .any(|code| item_status_upper == *code);

    // Scan candidates left-to-right for the first keyword hit.
    let mut keyword_match: Option<(&'static str, String, &'static str)> = None;
    for (field, value) in &candidates {
        let Some(text) = value else { continue };
        if text.trim().is_empty() {
            continue;
        }
        if let Some(kw) = VIOLATION_KEYWORDS.iter().find(|kw| text.contains(*kw)) {
            keyword_match = Some((field, text.clone(), kw));
            break;
        }
    }

    if !status_hit && keyword_match.is_none() {
        return None;
    }

    let (kind, reason): (String, String) = match (&keyword_match, status_hit) {
        (Some((_, text, kw)), _) => {
            let kind = classify_kind(kw, text);
            (kind, text.clone())
        }
        (None, true) => {
            // Status-code-only path — derive a kind from the code.
            let kind = match item_status_upper.as_str() {
                "AUDIT_FAILED" | "AUDIT_REJECTED" => "audit_failed",
                "BANNED" | "PUNISHED" | "ILLEGAL" => "banned",
                _ => "banned",
            };
            (kind.to_string(), format!("状态码: {item_status}"))
        }
        _ => unreachable!(),
    };

    // Build the raw-metadata snapshot. Only include keys that actually had a
    // value, to keep the JSON column small in the common case.
    let mut meta = Map::new();
    for (field, value) in &candidates {
        if let Some(text) = value {
            if !text.trim().is_empty() {
                meta.insert((*field).to_string(), Value::String(text.clone()));
            }
        }
    }
    if !item_status.trim().is_empty() {
        meta.insert("itemStatus".to_string(), Value::String(item_status.clone()));
    }
    let raw_metadata =
        serde_json::to_string(&Value::Object(meta)).unwrap_or_else(|_| "{}".to_string());

    Some(DetectedViolation {
        kind,
        reason,
        raw_metadata,
    })
}

/// Decide the violation bucket from the matched keyword. The `text` is kept
/// available for callers that want to add finer-grained heuristics later.
fn classify_kind(matched_keyword: &str, _text: &str) -> String {
    if matched_keyword.contains("审核") {
        "audit_failed".to_string()
    } else if matched_keyword.contains("投诉") {
        "complaint".to_string()
    } else if matched_keyword.contains("违规")
        || matched_keyword.contains("封禁")
        || matched_keyword.contains("处罚")
        || matched_keyword.contains("强制")
        || matched_keyword.contains("禁售")
        || matched_keyword.contains("涉嫌")
    {
        "banned".to_string()
    } else {
        "other".to_string()
    }
}

/// Extract a string at `card.<key>` for a single-element path. Returns
/// `None` when missing or empty.
fn string_at(card: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = card;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    match cursor {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Walk `card.banners[*].title` and `card.banner.title`, returning the first
/// non-empty title string.
fn banner_title(card: &Value) -> Option<String> {
    if let Some(banner) = card.get("banner") {
        if let Some(title) = banner.get("title").and_then(Value::as_str) {
            if !title.trim().is_empty() {
                return Some(title.to_string());
            }
        }
    }
    if let Some(banners) = card.get("banners").and_then(Value::as_array) {
        for banner in banners {
            if let Some(title) = banner.get("title").and_then(Value::as_str) {
                if !title.trim().is_empty() {
                    return Some(title.to_string());
                }
            }
        }
    }
    None
}

/// Compose the WeChat alert text for a single new violation.
fn render_violation_text(
    nick: &str,
    uid: &str,
    item_title: &str,
    item_id: &str,
    kind: &str,
    reason: &str,
) -> String {
    let nick_display = if nick.trim().is_empty() {
        "-"
    } else {
        nick.trim()
    };
    let title_display = if item_title.trim().is_empty() {
        "(无标题)"
    } else {
        item_title.trim()
    };
    let id_display = if item_id.trim().is_empty() {
        "-"
    } else {
        item_id
    };
    let reason_display = if reason.trim().is_empty() {
        kind
    } else {
        reason.trim()
    };
    format!(
        "## ⚠️ 检测到违规商品\n\n- 账号: **{nick_display}** (`{uid}`)\n- 商品: _{title_display}_\n- 商品ID: `{id_display}`\n- 原因: **{reason_display}**"
    )
}

/// Fire-and-forget WeChat push for ONE newly-inserted violation. Walks the
/// bind-all openclaw_weixin channels (same routing as the daily report) and
/// stamps `notified_at` on success.
///
/// Mirrors `dispatcher::dispatch_openclaw_weixin` for the actual HTTP send.
/// Kept in this module (not dispatcher.rs) because the trigger source is
/// different — dispatcher.rs is for IM message events, this is for sync-time
/// violation detection.
pub(super) async fn dispatch_violation_alert(
    state: AppState,
    violation_id: i64,
    uid: String,
    nick: String,
    item_id: String,
    item_title: String,
    violation_kind: String,
    reason_text: String,
) {
    let text = render_violation_text(
        &nick,
        &uid,
        &item_title,
        &item_id,
        &violation_kind,
        &reason_text,
    );

    let channels = match state
        .storage()
        .list_bind_all_notification_channels_for_uid(&uid)
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(uid = %uid, violation_id, error = %err, "violation alert: list channels failed");
            return;
        }
    };

    if channels.is_empty() {
        tracing::debug!(uid = %uid, violation_id, "violation alert: no bind-all channels configured, skipping push");
        return;
    }

    let mut any_success = false;
    for channel in channels {
        if !channel.enabled || channel.channel_type != "openclaw_weixin" {
            continue;
        }
        match send_via_openclaw_weixin(&channel, &text).await {
            Ok(()) => any_success = true,
            Err(err) => {
                tracing::warn!(
                    uid = %uid,
                    violation_id,
                    channel_id = channel.id,
                    error = %err,
                    "violation alert: openclaw_weixin send failed",
                );
            }
        }
    }

    if any_success {
        let ts = unix_millis_now();
        if let Err(err) = state
            .storage()
            .mark_violation_notified(violation_id, ts)
            .await
        {
            tracing::warn!(
                uid = %uid,
                violation_id,
                error = %err,
                "violation alert: failed to mark_violation_notified",
            );
        }
    }
}

/// Push a single text body to one channel via the iLink HTTP client. Returns
/// the error message on failure so the caller can log it without holding the
/// client's lifetime.
async fn send_via_openclaw_weixin(
    channel: &crate::models::notifications::NotificationChannel,
    text: &str,
) -> Result<(), String> {
    channel_send::send_text_to_channel(channel, text).await
}

fn config_str(config: &Map<String, Value>, key: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Fetch the seller's nick for a uid. Used only for prettier WeChat alert
/// formatting — falls back to empty string on miss, the alert formatter
/// renders "-" in that case.
pub(super) async fn fetch_account_nick(state: &AppState, uid: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT COALESCE(nick, '') FROM accounts WHERE uid = ?")
        .bind(uid)
        .fetch_optional(state.storage().accounts())
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn unix_millis_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canpolish_false_alone_is_not_a_violation() {
        let card = json!({
            "id": "123",
            "canPolish": false,
            "headTitle": "已售",
        });
        assert!(detect_violation(&card, "123").is_none());
    }

    #[test]
    fn explicit_violation_in_bizmessage() {
        let card = json!({
            "id": "123",
            "bizMessage": "违规商品下架",
        });
        let v = detect_violation(&card, "123").expect("expected hit");
        assert_eq!(v.kind, "banned");
        assert!(v.reason.contains("违规"));
    }

    #[test]
    fn audit_failed_in_banner() {
        let card = json!({
            "id": "9",
            "banner": { "title": "审核未通过，请修改后重新提交" },
        });
        let v = detect_violation(&card, "9").expect("expected hit");
        assert_eq!(v.kind, "audit_failed");
    }

    #[test]
    fn complaint_in_subtitle() {
        let card = json!({
            "id": "9",
            "subTitle": "投诉处理中，暂不可售",
        });
        let v = detect_violation(&card, "9").expect("expected hit");
        assert_eq!(v.kind, "complaint");
    }

    #[test]
    fn status_code_rejected() {
        let card = json!({
            "id": "9",
            "itemStatus": "REJECTED",
        });
        let v = detect_violation(&card, "9").expect("expected hit");
        assert_eq!(v.kind, "banned");
    }

    #[test]
    fn sold_out_only_is_not_a_violation() {
        let card = json!({
            "id": "9",
            "itemStatus": "SOLD_OUT",
            "headTitle": "已售",
            "canPolish": false,
        });
        assert!(detect_violation(&card, "9").is_none());
    }

    #[test]
    fn xiaoer_downshelf_alone_is_not_a_violation() {
        // "小二下架" without an explicit "违规" keyword is ambiguous (could be
        // a policy/activity end) — the items_sync header comment explicitly
        // calls this out. Do NOT flag.
        let card = json!({
            "id": "9",
            "headTitle": "小二下架",
        });
        assert!(detect_violation(&card, "9").is_none());
    }

    #[test]
    fn violation_text_uses_markdown_layout() {
        let text = render_violation_text(
            "测试账号",
            "123456",
            "测试商品",
            "9988",
            "banned",
            "涉嫌违规",
        );
        assert!(text.starts_with("## ⚠️ 检测到违规商品"));
        assert!(text.contains("- 账号: **测试账号** (`123456`)"));
        assert!(text.contains("- 商品: _测试商品_"));
        assert!(text.contains("- 商品ID: `9988`"));
        assert!(text.contains("- 原因: **涉嫌违规**"));
    }
}
