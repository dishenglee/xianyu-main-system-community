use axum::{
    extract::{Extension, Json, Path, Query, State},
    http::HeaderMap,
    routing::{get, patch, post},
    Json as ResponseJson, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{FromRow, Row};
use std::collections::HashMap;

use crate::{
    control_plane_tls::verify_response_pin,
    cookie_refresh,
    license_runtime::{apply_client_version_headers, get_sign_license_context},
    models::{auth::RequestIdentity, business::ReplyMode},
    routes::{
        operations::append_operation_log,
        tenant::{ensure_account_uid_accessible, ensure_admin_or_legacy, owner_username_filter},
    },
    state::AppState,
    xianyu_client::{self, LocationContext},
};

mod cloud_binding;
mod datacompass;
pub(crate) mod datacompass_metrics;
mod live;
pub(crate) mod messages;
mod publish_location;
mod qr;
pub(crate) mod runtime_access;
pub(crate) mod shared;
pub mod summary;
mod wx_live;

pub(crate) use live::{
    decode_and_process_live_sync_payload, parse_ws_json_frame, try_process_fallback_live_event,
};
// Re-export `LiveMessageEvent` for the notifications dispatcher and
// integration tests without making the `messages` module itself public.
pub use messages::prefetch_account_message_history;
pub use messages::LiveMessageEvent;
use messages::{
    coerce_mtop_data, json_string, live_create_chat, live_mark_session_read, live_recall_message,
    live_send_image, live_send_item, live_send_location, nearby_pois, pc_mark_session_read,
    pc_message_headinfo, pc_message_history, pc_send_message, pc_session_info, pc_sessions,
    pc_sessions_batch, search_items_for_send, search_pois,
};
pub(crate) use messages::{LiveSendImagePayload, PcSendPayload};
use runtime_access::{
    attach_runtime_access_fields, ensure_account_can_call_upstream, load_account_owner_username,
    resolve_accounts_runtime_access,
};
pub(crate) use wx_live::{
    do_wx_live_session_keepalive, fetch_wx_live_access_token, generate_live_mid,
    wx_live_ws_connect_timeout, wx_live_ws_url,
};

pub(crate) use qr::start_qr_session_for_command;
use qr::{qr_generate, qr_status};
use shared::{
    default_page, default_page_size_20, default_page_size_50, default_recent_30d, parse_json_array,
    parse_json_object, qr_sessions, unix_millis_now, unix_seconds_now, ApiResult, QrSessionState,
};
pub(crate) use shared::{
    first_ret_string, payload_ret_success, resolve_account_protocol,
    successful_upstream_app_request, successful_upstream_h5_request,
    successful_upstream_protocol_app_request, successful_upstream_protocol_h5_request,
    successful_upstream_protocol_wx_request, successful_upstream_wx_request, upstream_h5_request,
    upstream_protocol_h5_request,
};
use summary::{
    account_alipay, account_coin_balance, account_coin_detail, account_extra_map, account_profile,
    account_profile_editable, account_profile_update, account_qualification,
    account_row_to_value_with_publish_location, account_security_detection, account_security_score,
    account_security_score_records, account_seller_data, account_summary, account_user_check,
    account_user_head, account_user_page, account_violations, account_violations_all, coin_account,
    coin_info, load_account_row, persist_account_extra,
};

pub(crate) use shared::ResponseError;

const BOOTSTRAP_REFRESH_INTERVAL: i64 = 6000;
#[allow(non_snake_case)]
fn QR_DEFAULT_PASSPORT_HOST() -> &'static str {
    crate::sealed_protocol::sealed_str!("https://passport.goofish.com")
}
#[allow(non_snake_case)]
fn QR_DEFAULT_RENDER_REFERER() -> &'static str {
    crate::sealed_protocol::sealed_str!("https://www.goofish.com/")
}
#[allow(non_snake_case)]
fn QR_DEFAULT_LOGIN_BIZ_PARAMS() -> &'static str {
    crate::sealed_protocol::sealed_str!(
        "taobaoBizLoginFrom=web&renderRefer=https%3A%2F%2Fwww.goofish.com%2F"
    )
}
const QR_BAXIA_VERSION: &str = "2.5.31";
#[allow(non_snake_case)]
fn WX_LIVE_MTOP_GATEWAY() -> &'static str {
    crate::sealed_protocol::sealed_str!("https://acs.m.goofish.com")
}
const WX_LIVE_WS_URL: &str = "wss://wss-goofish.dingtalk.com/";
pub(crate) const WX_LIVE_ACCS_APP_KEY: &str = "ba4abbf9fee7a986f0dbe536e8f3f78f";
const WX_LIVE_MTOP_APP_KEY: &str = "12574478";
const WX_LIVE_JSV: &str = "2.4.12";
const WX_LIVE_REFERER: &str = "https://servicewechat.com/wx9882f2a891880616/76/page-frame.html";
pub(crate) const WX_LIVE_ORIGIN: &str = "https://servicewechat.com";
pub(crate) const WX_LIVE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 26_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 MicroMessenger/8.0.70(0x18004639) NetType/WIFI Language/zh_CN";
const WX_LIVE_UTDID: &str = "28b8IGorElQCAQAAAACuy6pp";
const WX_LIVE_MINI_APP_VERSION: &str = "0.3.78";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/accounts"),
            get(list_accounts),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/accounts/health"),
            get(accounts_health),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/accounts/:uid/location"),
            post(update_account_location),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/accounts/:uid"),
            patch(update_account_settings).delete(delete_account),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/accounts/:uid/refresh"),
            post(refresh_account),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/accounts/unbind-requests"),
            post(create_account_unbind_request),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/admin/account-unbind-requests"),
            get(list_account_unbind_requests),
        )
        .route(
            crate::sealed_protocol::sealed_str!(
                "/admin/account-unbind-requests/:request_id/approve"
            ),
            post(approve_account_unbind_request),
        )
        .route(
            crate::sealed_protocol::sealed_str!(
                "/admin/account-unbind-requests/:request_id/reject"
            ),
            post(reject_account_unbind_request),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/accounts/refresh-all"),
            post(refresh_all_accounts),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/accounts/:uid/wx-device-id/rotate"),
            post(rotate_wx_device_id),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/qr/generate"),
            post(qr_generate),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/qr/status/:session_id"),
            get(qr_status),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/summary"),
            get(account_summary),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/profile"),
            get(account_profile).post(account_profile_update),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/profile/editable"),
            get(account_profile_editable),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/security_score"),
            get(account_security_score),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/security_score/records"),
            get(account_security_score_records),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/violations"),
            get(account_violations),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/violations/all"),
            get(account_violations_all),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/security_detection"),
            get(account_security_detection),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/qualification"),
            get(account_qualification),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/coin_balance"),
            get(account_coin_balance),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/coin_detail"),
            get(account_coin_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/seller_data"),
            get(account_seller_data),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/user_page"),
            get(account_user_page),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/user_head"),
            get(account_user_head),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/user_check"),
            get(account_user_check),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/alipay"),
            get(account_alipay),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/info"),
            get(coin_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/account"),
            get(coin_account),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/pc/sessions/batch"),
            get(pc_sessions_batch),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/pc/sessions"),
            get(pc_sessions),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/pc/history"),
            get(pc_message_history),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/pc/session-info"),
            get(pc_session_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/pc/headinfo"),
            get(pc_message_headinfo),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/pc/send"),
            post(pc_send_message),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/pc/sessions/read"),
            post(pc_mark_session_read),
        )
        // Live ACCS WS RPC routes (Python parity: `XianyuWxLiveClient`):
        //   create_chat — `/r/SingleChatConversation/create` with CID waiter
        //   send_image  — `/r/MessageSend/sendByReceiverScope` with custom.type=2
        // Both require an active live socket for `uid` (503 otherwise).
        .route(
            crate::sealed_protocol::sealed_str!("/messages/chat/create"),
            post(live_create_chat),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/send/image"),
            post(live_send_image),
        )
        // Rich-card sends (see `messages::live_ops_cards`):
        //   send/location — `contentType=30` locationCard (lat/lng + title/content +
        //     static alicdn thumb). UI shows a map-pin bubble.
        //   send/item     — `contentType=7`  itemCard (商品卡片) for sharing a
        //     listing into chat. Bubble shows pic + title + price + "我想要" CTA.
        // Both register a `live_response_waiters` slot so the PNM messageId from
        // the ACK gets stashed in the optimistic message row (required for recall).
        .route(
            crate::sealed_protocol::sealed_str!("/messages/send/location"),
            post(live_send_location),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/messages/send/item"),
            post(live_send_item),
        )
        // Item-card picker source. Wraps
        // `mtop.taobao.idlemessage.pc.tool.item.search` so the composer can
        // list either the current account's listings (role=own) or the peer's
        // listings (role=other) — the latter has no `GET /api/items` flavor
        // because items are looked up by the conversation sessionId upstream.
        .route(
            crate::sealed_protocol::sealed_str!("/messages/items/search"),
            post(search_items_for_send),
        )
        // 附近 POI 代理 — 调高德 v3/place/around。前端直连受 CSP/同源限制，
        // 搬到后端就没这些麻烦。lat/lng 由前端 navigator.geolocation 取，POST 过来。
        .route(
            crate::sealed_protocol::sealed_str!("/messages/location/nearby"),
            post(nearby_pois),
        )
        // 高德 inputtips 代理 —— 用户在搜索框打字时调，前端给关键词，后端返
        // 匹配的 POI 列表（带坐标）。跟 nearby 同样的 referrer 锁问题。
        .route(
            crate::sealed_protocol::sealed_str!("/messages/location/search"),
            post(search_pois),
        )
        // Read-receipts: pushes `mtop.taobao.idlemessage.relation.message.read`
        // upstream + zeroes the local unread count. Frontend calls this when
        // the user focuses a chat session.
        .route(
            crate::sealed_protocol::sealed_str!("/messages/session/read"),
            post(live_mark_session_read),
        )
        // Message recall (撤回). Best-effort LWP push +
        // local rewrite of the cached row to `kind="recalled"`. See
        // `messages::read_recall` for the catalog-blank caveat.
        .route(
            crate::sealed_protocol::sealed_str!("/messages/recall"),
            post(live_recall_message),
        )
        .merge(publish_location::routes())
        .merge(datacompass::routes())
}

#[derive(Debug, Deserialize)]
struct StatusQuery {
    #[serde(default)]
    status: String,
}

#[derive(Debug, Deserialize)]
struct UidQuery {
    uid: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AccountSettingsPatch {
    #[serde(default)]
    reply_mode: Option<ReplyMode>,
    #[serde(default)]
    auto_reply: Option<bool>,
    #[serde(default)]
    auto_ship: Option<bool>,
    #[serde(default)]
    ai_enabled: Option<bool>,
    #[serde(default)]
    auto_recall_item_replies: Option<bool>,
    #[serde(default)]
    app_request_headers: Option<Map<String, Value>>,
}

fn reply_mode_from_account_extra(extra: &Map<String, Value>) -> ReplyMode {
    if let Some(raw) = extra.get("replyMode").and_then(Value::as_str) {
        return ReplyMode::from_account_raw(raw);
    }
    if extra
        .get("aiEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ReplyMode::Ai;
    }
    if extra
        .get("autoReply")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ReplyMode::Default;
    }
    ReplyMode::Off
}

#[derive(Debug, Deserialize)]
struct AccountSummaryQuery {
    uid: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct PageQuery {
    uid: String,
    #[serde(default = "default_page")]
    page: i64,
}

#[derive(Debug, Deserialize)]
struct ViolationsAllQuery {
    uid: String,
    #[serde(default = "default_page_size_50")]
    page_size: i64,
}

#[derive(Debug, Deserialize)]
struct CoinDetailQuery {
    uid: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size_20")]
    page_size: i64,
}

#[derive(Debug, Deserialize)]
struct SellerDataQuery {
    uid: String,
    #[serde(default = "default_recent_30d")]
    date_type: String,
}

async fn list_accounts(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<StatusQuery>,
) -> ApiResult {
    let owner = account_owner_filter(&state, &identity);
    let status = query.status.trim();
    let sql = if status.is_empty() {
        r#"
        SELECT uid, nick, token, sid, cookies, browser_cookie_jar, status, created_at, updated_at, bound_at,
               cookie_refreshed_at, last_session_check_at, last_session_ok_at, first_session_fail_at,
               last_session_status, last_session_reason, extra, security_score, security_score_updated_at,
               restriction_end, coin_balance, violation_count, fish_power, total_earned,
               fish_shop_enabled, fish_shop_level, seller_30d_sold
        FROM accounts
        WHERE (? = '' OR owner_username = ?)
        ORDER BY bound_at DESC, updated_at DESC, uid ASC
        "#
    } else {
        r#"
        SELECT uid, nick, token, sid, cookies, browser_cookie_jar, status, created_at, updated_at, bound_at,
               cookie_refreshed_at, last_session_check_at, last_session_ok_at, first_session_fail_at,
               last_session_status, last_session_reason, extra, security_score, security_score_updated_at,
               restriction_end, coin_balance, violation_count, fish_power, total_earned,
               fish_shop_enabled, fish_shop_level, seller_30d_sold
        FROM accounts
        WHERE status = ? AND (? = '' OR owner_username = ?)
        ORDER BY bound_at DESC, updated_at DESC, uid ASC
        "#
    };

    let rows = if status.is_empty() {
        sqlx::query(sql)
            .bind(&owner)
            .bind(&owner)
            .fetch_all(state.storage().accounts())
            .await
            .map_err(ResponseError::internal)?
    } else {
        sqlx::query(sql)
            .bind(status)
            .bind(&owner)
            .bind(&owner)
            .fetch_all(state.storage().accounts())
            .await
            .map_err(ResponseError::internal)?
    };

    let live_states = state
        .storage()
        .list_live_connection_states()
        .await
        .map_err(ResponseError::internal)?;
    let live_state_by_uid = live_states
        .into_iter()
        .map(|entry| (entry.uid.clone(), entry))
        .collect::<HashMap<_, _>>();
    let account_uids = rows
        .iter()
        .map(|row| row.get::<String, _>("uid"))
        .collect::<Vec<_>>();
    let runtime_access_by_uid = match resolve_accounts_runtime_access(&state, &account_uids).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(error = %error, "failed to resolve remote account runtime access");
            HashMap::new()
        }
    };

    let mut accounts = Vec::with_capacity(rows.len());
    for row in rows {
        let uid = row.get::<String, _>("uid");
        let mut account =
            account_row_to_value_with_publish_location(state.storage().accounts(), row).await;
        let pending_delivery_mismatch_count = if account
            .get("autoShip")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            match state
                .storage()
                .pending_delivery_order_mismatches(&uid)
                .await
            {
                Ok(mismatches) => mismatches.len(),
                Err(error) => {
                    tracing::warn!(uid, error = %error, "读取账号自动发货异常失败");
                    0
                }
            }
        } else {
            0
        };
        if let Some(object) = account.as_object_mut() {
            object.insert(
                "pendingDeliveryMismatchCount".to_string(),
                json!(pending_delivery_mismatch_count),
            );
            object.insert(
                "pending_delivery_mismatch_count".to_string(),
                json!(pending_delivery_mismatch_count),
            );
            if let Some(live_state) = live_state_by_uid.get(&uid) {
                object.insert(
                    "liveStatus".to_string(),
                    Value::String(live_state.status.clone()),
                );
                object.insert(
                    "live_status".to_string(),
                    Value::String(live_state.status.clone()),
                );
                object.insert(
                    "liveLastConnectedAt".to_string(),
                    json!(live_state.last_connected_at),
                );
                object.insert(
                    "live_last_connected_at".to_string(),
                    json!(live_state.last_connected_at),
                );
                object.insert(
                    "liveLastDisconnectedAt".to_string(),
                    json!(live_state.last_disconnected_at),
                );
                object.insert(
                    "live_last_disconnected_at".to_string(),
                    json!(live_state.last_disconnected_at),
                );
                object.insert(
                    "liveLastError".to_string(),
                    Value::String(live_state.last_error.clone()),
                );
                object.insert(
                    "live_last_error".to_string(),
                    Value::String(live_state.last_error.clone()),
                );
                object.insert("liveUpdatedAt".to_string(), json!(live_state.updated_at));
                object.insert("live_updated_at".to_string(), json!(live_state.updated_at));
            } else {
                object.insert("liveStatus".to_string(), Value::String(String::new()));
                object.insert("live_status".to_string(), Value::String(String::new()));
            }
            if let Some(access) = runtime_access_by_uid.get(&uid) {
                attach_runtime_access_fields(object, access);
            }
        }
        accounts.push(account);
    }
    Ok(ResponseJson(Value::Array(accounts)))
}

async fn update_account_location(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
    Json(payload): Json<Value>,
) -> ApiResult {
    ensure_account_access(&state, &identity, &uid).await?;
    let body = payload.as_object().cloned().unwrap_or_default();
    let mut extra = account_extra_map(state.storage().accounts(), &uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;

    let mut location = Map::new();
    for key in [
        "fishAdCode",
        "gps",
        "latitude",
        "longitude",
        "city",
        "province",
        "area",
        "divisionId",
    ] {
        if let Some(value) = body.get(key) {
            location.insert(key.to_string(), value.clone());
        }
    }
    if location.contains_key("latitude")
        && location.contains_key("longitude")
        && !location.contains_key("gps")
    {
        let latitude = location
            .get("latitude")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let longitude = location
            .get("longitude")
            .and_then(Value::as_str)
            .unwrap_or_default();
        location.insert(
            "gps".to_string(),
            Value::String(format!("{latitude},{longitude}")),
        );
    }

    extra.insert("location".to_string(), Value::Object(location.clone()));
    persist_account_extra(state.storage().accounts(), &uid, &extra).await?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "account",
            action: "update_location",
            operator: &uid,
            trigger_source: "manual",
            uid: &uid,
            account_nick: "",
            target_type: "account",
            target_id: &uid,
            target_title: "",
            status: "success",
            error_code: "",
            message: "更新账号发布地成功",
            params: Value::Object(body.clone()),
            before: json!({}),
            after: Value::Object(location.clone()),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "location": location,
    })))
}

async fn update_account_settings(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
    Json(payload): Json<AccountSettingsPatch>,
) -> ApiResult {
    ensure_account_access(&state, &identity, &uid).await?;
    let mut extra = account_extra_map(state.storage().accounts(), &uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;

    let reply_mode = payload.reply_mode;
    let auto_reply = payload.auto_reply;
    let auto_ship = payload.auto_ship;
    let ai_enabled = payload.ai_enabled;
    let auto_recall_item_replies = payload.auto_recall_item_replies;
    let app_request_headers = payload.app_request_headers;

    let mut ai_patch = crate::models::business::AiSettingsUpdatePayload::default();
    ai_patch.reply_mode = reply_mode;
    ai_patch.auto_reply_enabled = auto_reply;
    ai_patch.auto_ship_enabled = auto_ship;
    ai_patch.ai_enabled = ai_enabled;
    let updated_settings = if ai_patch.reply_mode.is_some()
        || ai_patch.auto_reply_enabled.is_some()
        || ai_patch.auto_ship_enabled.is_some()
        || ai_patch.ai_enabled.is_some()
    {
        Some(
            state
                .storage()
                .upsert_ai_settings(&uid, &ai_patch)
                .await
                .map_err(ResponseError::internal)?,
        )
    } else {
        None
    };

    let effective_reply_mode = updated_settings
        .as_ref()
        .map(|settings| settings.reply_mode.normalize_account())
        .unwrap_or_else(|| reply_mode_from_account_extra(&extra));

    extra.insert(
        "replyMode".to_string(),
        Value::String(effective_reply_mode.as_str().to_string()),
    );
    extra.insert(
        "autoReply".to_string(),
        Value::Bool(effective_reply_mode.auto_reply_enabled()),
    );
    extra.insert(
        "aiEnabled".to_string(),
        Value::Bool(effective_reply_mode.ai_enabled()),
    );
    if let Some(value) = auto_ship {
        extra.insert("autoShip".to_string(), Value::Bool(value));
    }
    if let Some(value) = auto_recall_item_replies {
        extra.insert("autoRecallItemReplies".to_string(), Value::Bool(value));
    }
    if let Some(value) = app_request_headers.clone() {
        extra.insert("appRequestHeaders".to_string(), Value::Object(value));
    }
    persist_account_extra(state.storage().accounts(), &uid, &extra).await?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "account",
            action: "update_settings",
            operator: &uid,
            trigger_source: "manual",
            uid: &uid,
            account_nick: "",
            target_type: "account",
            target_id: &uid,
            target_title: "",
            status: "success",
            error_code: "",
            message: "更新账号设置成功",
            params: json!({
                "replyMode": reply_mode,
                "autoReply": auto_reply,
                "autoShip": auto_ship,
                "aiEnabled": ai_enabled,
                "autoRecallItemReplies": auto_recall_item_replies,
                "appRequestHeaders": app_request_headers,
            }),
            before: json!({}),
            after: Value::Object(extra.clone()),
            extra: json!({}),
        },
    )
    .await;

    let row = load_account_row(state.storage().accounts(), &uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    Ok(ResponseJson(
        account_row_to_value_with_publish_location(state.storage().accounts(), row).await,
    ))
}

async fn delete_account(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
) -> ApiResult {
    ensure_account_access(&state, &identity, &uid).await?;
    let row_before = load_account_row(state.storage().accounts(), &uid)
        .await
        .map_err(ResponseError::internal)?;
    let Some(row_before) = row_before else {
        return Err(ResponseError::not_found("账号不存在"));
    };
    let before_value =
        account_row_to_value_with_publish_location(state.storage().accounts(), row_before).await;
    let deleted = sqlx::query("DELETE FROM accounts WHERE uid = ?")
        .bind(&uid)
        .execute(state.storage().accounts())
        .await
        .map_err(ResponseError::internal)?
        .rows_affected();
    if deleted == 0 {
        return Err(ResponseError::not_found("账号不存在"));
    }
    sqlx::query("DELETE FROM live_connection_state WHERE uid = ?")
        .bind(&uid)
        .execute(state.storage().live_state())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM message_live_state WHERE uid = ?")
        .bind(&uid)
        .execute(state.storage().live_state())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM message_live_event WHERE uid = ?")
        .bind(&uid)
        .execute(state.storage().live_state())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM message_history_cache WHERE uid = ?")
        .bind(&uid)
        .execute(state.storage().live_state())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM account_notification_bindings WHERE uid = ?")
        .bind(&uid)
        .execute(state.storage().notifications())
        .await
        .map_err(ResponseError::internal)?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "account",
            action: "delete_account",
            operator: &uid,
            trigger_source: "manual",
            uid: &uid,
            account_nick: "",
            target_type: "account",
            target_id: &uid,
            target_title: "",
            status: "success",
            error_code: "",
            message: "删除账号成功",
            params: json!({}),
            before: before_value,
            after: json!({
                "deleted": true,
            }),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({ "status": "ok" })))
}

async fn refresh_account(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
) -> ApiResult {
    ensure_account_access(&state, &identity, &uid).await?;
    let owner_username = load_account_owner_username(&state, &uid).await?;
    ensure_account_can_call_upstream(&state, owner_username.as_deref(), &uid).await?;

    // 用深度刷新：bootstrap_h5tk + http_resume_haslogin_session。
    // 后者是关键 —— 续期 unb/sgcookie/cookie2 让 live 模块的 wx.login.token
    // 不再报 FAIL_SYS_TOKEN_ILLEGAL。
    let outcome = cookie_refresh::refresh_account_cookies_deep(state.storage().accounts(), &uid)
        .await
        .map_err(ResponseError::internal_message)?;

    // Python parity: after the token/base refresh, probe a real business
    // MTOP so the UI reports "真的能用" rather than "token 已写回". Surfaces
    // `ret_code` + probe steps so operator can see exactly what the ladder
    // had to do (and whether it ran a resume/reanchor round).
    let probe = if outcome.ok {
        match cookie_refresh::probe_and_repair_business_session(
            state.config(),
            state.storage().accounts(),
            &uid,
        )
        .await
        {
            Ok(probe) => Some(probe),
            Err(err) => {
                tracing::warn!(uid = %uid, error = %err, "probe errored after manual refresh");
                None
            }
        }
    } else {
        None
    };

    let (probe_ok, probe_kind, probe_ret, probe_steps) = match &probe {
        Some(p) => (
            p.ok,
            p.management_kind.tag().to_string(),
            p.ret_code.clone(),
            p.steps.clone(),
        ),
        None => (false, "skipped".to_string(), String::new(), Vec::new()),
    };

    if outcome.ok && !probe_ok && !probe_ret.trim().is_empty() {
        let _ = cookie_refresh::mark_business_session_failure(
            state.storage().accounts(),
            &uid,
            &format!("manual_refresh_probe_failed: {probe_ret}"),
        )
        .await;
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "account",
            action: "refresh_account",
            operator: &uid,
            trigger_source: "manual",
            uid: &uid,
            account_nick: "",
            target_type: "account",
            target_id: &uid,
            target_title: "",
            status: if outcome.ok && probe_ok {
                "success"
            } else {
                "failed"
            },
            error_code: if outcome.ok && probe_ok {
                ""
            } else {
                probe_ret.as_str()
            },
            message: if outcome.ok && probe_ok {
                "刷新账号成功"
            } else {
                "刷新账号失败"
            },
            params: json!({}),
            before: json!({}),
            after: json!({
                "ok": outcome.ok && probe_ok,
                "detail": outcome.detail,
                "reason": outcome.reason,
                "probe": {
                    "ok": probe_ok,
                    "kind": probe_kind,
                    "ret_code": probe_ret,
                    "steps": probe_steps,
                },
            }),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": if outcome.ok && probe_ok { "ok" } else { "error" },
        "ok": outcome.ok && probe_ok,
        "uid": uid,
        "detail": outcome.detail,
        "reason": outcome.reason,
        "refreshed_at": outcome.refreshed_at,
        "probe": {
            "ok": probe_ok,
            "kind": probe_kind,
            "ret_code": probe_ret,
            "steps": probe_steps,
        },
    })))
}

async fn refresh_all_accounts(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let owner = account_owner_filter(&state, &identity);
    let rows = sqlx::query(
        r#"
        SELECT uid FROM accounts
        WHERE status != 'disabled' AND (? = '' OR owner_username = ?)
        ORDER BY uid
        "#,
    )
    .bind(&owner)
    .bind(&owner)
    .fetch_all(state.storage().accounts())
    .await
    .map_err(ResponseError::internal)?;

    let mut results = Vec::with_capacity(rows.len());
    let mut ok_count = 0usize;
    let mut fail_count = 0usize;
    for row in rows {
        let uid: String = row.get("uid");
        let owner_username = load_account_owner_username(&state, &uid).await?;
        if let Err(error) =
            ensure_account_can_call_upstream(&state, owner_username.as_deref(), &uid).await
        {
            fail_count += 1;
            results.push(json!({
                "uid": uid,
                "ok": false,
                "detail": error.detail(),
                "reason": "remote_binding_restricted",
                "refreshed_at": 0.0,
            }));
            continue;
        }
        match cookie_refresh::refresh_account_cookies_deep(state.storage().accounts(), &uid).await {
            Ok(outcome) => {
                if outcome.ok {
                    ok_count += 1;
                } else {
                    fail_count += 1;
                }
                results.push(json!({
                    "uid": uid,
                    "ok": outcome.ok,
                    "detail": outcome.detail,
                    "reason": outcome.reason,
                    "refreshed_at": outcome.refreshed_at,
                }));
            }
            Err(err) => {
                fail_count += 1;
                results.push(json!({
                    "uid": uid,
                    "ok": false,
                    "detail": err,
                    "reason": "refresh_error",
                    "refreshed_at": 0.0,
                }));
            }
        }
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "account",
            action: "refresh_all_accounts",
            operator: "system",
            trigger_source: "manual",
            uid: "",
            account_nick: "",
            target_type: "account_batch",
            target_id: "refresh-all",
            target_title: "",
            status: if fail_count == 0 {
                "success"
            } else {
                "partial_success"
            },
            error_code: "",
            message: "批量刷新账号完成",
            params: json!({}),
            before: json!({}),
            after: json!({
                "total": results.len(),
                "ok_count": ok_count,
                "fail_count": fail_count,
            }),
            extra: json!(results),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "total": results.len(),
        "ok_count": ok_count,
        "fail_count": fail_count,
        "results": results,
    })))
}

/// POST /api/accounts/:uid/wx-device-id/rotate
/// Force-rotate the persisted ACCS device_id so the next reconnect picks up
/// a fresh server-side reputation. Use this for accounts that get stuck in
/// permanent cycling — once we tear down the live worker, the new device_id
/// goes out on the very next /reg.
async fn rotate_wx_device_id(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
) -> ApiResult {
    ensure_account_access(&state, &identity, &uid).await?;
    let new_did = wx_live::regenerate_wx_device_id(state.storage().accounts(), &uid)
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(json!({
        "uid": uid,
        "wx_device_id": new_did,
        "note": "device_id rotated; next ACCS reconnect for this uid will use the new value",
    })))
}

async fn accounts_health(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let owner = account_owner_filter(&state, &identity);
    let rows = sqlx::query(
        r#"
        SELECT uid, status, token, cookies, last_session_status, last_session_reason,
               security_score, coin_balance, fish_power
        FROM accounts
        WHERE (? = '' OR owner_username = ?)
        ORDER BY updated_at DESC, uid ASC
        "#,
    )
    .bind(&owner)
    .bind(&owner)
    .fetch_all(state.storage().accounts())
    .await
    .map_err(ResponseError::internal)?;

    let results = rows
        .into_iter()
        .map(|row| {
            let cookies = parse_json_object(row.get::<String, _>("cookies").as_str());
            let has_h5_tk =
                row.get::<String, _>("token").trim().len() > 0 || cookies.contains_key("_m_h5_tk");
            let ready = row.get::<i64, _>("security_score") >= 0
                || row.get::<i64, _>("coin_balance") > 0
                || row.get::<i64, _>("fish_power") > 0;
            json!({
                "uid": row.get::<String, _>("uid"),
                "healthy": has_h5_tk || ready,
                "status": row.get::<String, _>("status"),
                "reason": if has_h5_tk || ready {
                    "ok".to_string()
                } else {
                    row.get::<String, _>("last_session_reason")
                },
                "last_session_status": row.get::<String, _>("last_session_status"),
            })
        })
        .collect::<Vec<_>>();

    Ok(ResponseJson(json!(results)))
}

fn account_owner_filter(state: &AppState, identity: &RequestIdentity) -> String {
    owner_username_filter(state, identity).unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct AccountUnbindRequestPayload {
    account_uid: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountUnbindReviewPayload {
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccountUnbindListQuery {
    #[serde(default)]
    status: String,
    #[serde(default = "default_account_unbind_review_limit")]
    limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct AccountSnapshot {
    uid: String,
    nick: String,
    token: String,
    sid: String,
    x_umt: String,
    cookies: String,
    browser_cookie_jar: String,
    app_info: String,
    status: String,
    created_at: f64,
    updated_at: f64,
    bound_at: f64,
    cookie_refreshed_at: f64,
    last_session_check_at: f64,
    last_session_ok_at: f64,
    first_session_fail_at: f64,
    last_session_status: String,
    last_session_reason: String,
    last_session_evidence: String,
    extra: String,
    security_score: i64,
    security_score_updated_at: f64,
    restriction_end: String,
    coin_balance: i64,
    violation_count: i64,
    fish_power: i64,
    total_earned: f64,
    fish_shop_enabled: i64,
    fish_shop_level: String,
    seller_30d_sold: String,
    owner_username: String,
    wx_device_id: String,
}

#[derive(Debug, Clone, FromRow)]
struct AccountUnbindRequestRow {
    id: i64,
    account_uid: String,
    requester_username: String,
    existing_owner_username: String,
    status: String,
    reason: String,
    requested_at: f64,
    updated_at: f64,
    reviewed_by: String,
    reviewed_at: f64,
    review_note: String,
    account_snapshot: String,
}

fn default_account_unbind_review_limit() -> i64 {
    50
}

fn license_auth_token_header() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-auth-token")
}

pub(super) fn license_server_url(state: &AppState) -> String {
    get_sign_license_context()
        .map(|context| context.server_url)
        .unwrap_or_else(|| state.config().xianyu.license_server_url.clone())
        .trim()
        .trim_end_matches('/')
        .to_string()
}

pub(super) fn license_auth_token_from_headers_or_context(headers: &HeaderMap) -> Option<String> {
    headers
        .get(license_auth_token_header())
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            get_sign_license_context()
                .map(|context| context.auth_token)
                .filter(|value| !value.trim().is_empty())
        })
}

pub(super) async fn license_response_error(response: reqwest::Response) -> ResponseError {
    let status = response.status();
    let detail = response
        .json::<Value>()
        .await
        .ok()
        .and_then(|value| {
            value
                .get("detail")
                .and_then(Value::as_str)
                .or_else(|| value.get("reason").and_then(Value::as_str))
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("鉴权服务返回 {status}"));
    if status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::CONFLICT {
        ResponseError::private(axum::http::StatusCode::BAD_REQUEST, detail)
    } else if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
    {
        ResponseError::forbidden("远程授权会话无效，请重新登录后再申请解绑")
    } else {
        ResponseError::private(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            format!("鉴权服务异常：{detail}"),
        )
    }
}

fn normalize_unbind_text(value: Option<&str>, fallback: &str) -> String {
    let text = value.unwrap_or(fallback).trim();
    let normalized = if text.is_empty() { fallback } else { text };
    normalized.chars().take(500).collect()
}

fn account_unbind_request_row_to_value(row: &AccountUnbindRequestRow) -> Value {
    json!({
        "id": row.id,
        "account_uid": row.account_uid,
        "requester_username": row.requester_username,
        "existing_owner_username": row.existing_owner_username,
        "status": row.status,
        "reason": row.reason,
        "requested_at": row.requested_at,
        "updated_at": row.updated_at,
        "reviewed_by": row.reviewed_by,
        "reviewed_at": row.reviewed_at,
        "review_note": row.review_note,
    })
}

async fn load_account_snapshot(
    executor: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    account_uid: &str,
) -> Result<Option<AccountSnapshot>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            uid, nick, token, sid, x_umt, cookies, browser_cookie_jar, app_info, status,
            created_at, updated_at, bound_at, cookie_refreshed_at, last_session_check_at,
            last_session_ok_at, first_session_fail_at, last_session_status, last_session_reason,
            last_session_evidence, extra, security_score, security_score_updated_at,
            restriction_end, coin_balance, violation_count, fish_power, total_earned,
            fish_shop_enabled, fish_shop_level, seller_30d_sold, owner_username, wx_device_id
        FROM accounts
        WHERE uid = ?
        LIMIT 1
        "#,
    )
    .bind(account_uid.trim())
    .fetch_optional(executor)
    .await
}

async fn upsert_pending_local_account_unbind_request(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_uid: &str,
    requester: &str,
    existing_owner: &str,
    reason: &str,
    snapshot_json: &str,
) -> Result<i64, ResponseError> {
    let now = unix_seconds_now();
    let existing: Option<(i64, String)> = sqlx::query_as(
        r#"
        SELECT id, account_snapshot
        FROM account_unbind_requests
        WHERE account_uid = ? AND requester_username = ? AND status = 'pending'
        LIMIT 1
        "#,
    )
    .bind(account_uid.trim())
    .bind(requester.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(ResponseError::internal)?;

    if let Some((request_id, existing_snapshot)) = existing {
        let snapshot_to_store = if existing_snapshot.trim().is_empty() || existing_snapshot == "{}"
        {
            snapshot_json
        } else {
            existing_snapshot.as_str()
        };
        sqlx::query(
            r#"
            UPDATE account_unbind_requests
            SET existing_owner_username = ?,
                reason = ?,
                requested_at = ?,
                updated_at = ?,
                account_snapshot = ?
            WHERE id = ?
            "#,
        )
        .bind(existing_owner.trim())
        .bind(reason)
        .bind(now)
        .bind(now)
        .bind(snapshot_to_store)
        .bind(request_id)
        .execute(&mut **tx)
        .await
        .map_err(ResponseError::internal)?;
        return Ok(request_id);
    }

    let result = sqlx::query(
        r#"
        INSERT INTO account_unbind_requests (
            account_uid,
            requester_username,
            existing_owner_username,
            status,
            reason,
            requested_at,
            updated_at,
            account_snapshot
        )
        VALUES (?, ?, ?, 'pending', ?, ?, ?, ?)
        "#,
    )
    .bind(account_uid.trim())
    .bind(requester.trim())
    .bind(existing_owner.trim())
    .bind(reason)
    .bind(now)
    .bind(now)
    .bind(snapshot_json)
    .execute(&mut **tx)
    .await
    .map_err(ResponseError::internal)?;

    Ok(result.last_insert_rowid())
}

async fn restore_account_snapshot(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    snapshot: &AccountSnapshot,
) -> Result<(), ResponseError> {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid, nick, token, sid, x_umt, cookies, browser_cookie_jar, app_info, status,
            created_at, updated_at, bound_at, cookie_refreshed_at, last_session_check_at,
            last_session_ok_at, first_session_fail_at, last_session_status, last_session_reason,
            last_session_evidence, extra, security_score, security_score_updated_at,
            restriction_end, coin_balance, violation_count, fish_power, total_earned,
            fish_shop_enabled, fish_shop_level, seller_30d_sold, owner_username, wx_device_id
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(uid) DO UPDATE SET
            nick = excluded.nick,
            token = excluded.token,
            sid = excluded.sid,
            x_umt = excluded.x_umt,
            cookies = excluded.cookies,
            browser_cookie_jar = excluded.browser_cookie_jar,
            app_info = excluded.app_info,
            status = excluded.status,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            bound_at = excluded.bound_at,
            cookie_refreshed_at = excluded.cookie_refreshed_at,
            last_session_check_at = excluded.last_session_check_at,
            last_session_ok_at = excluded.last_session_ok_at,
            first_session_fail_at = excluded.first_session_fail_at,
            last_session_status = excluded.last_session_status,
            last_session_reason = excluded.last_session_reason,
            last_session_evidence = excluded.last_session_evidence,
            extra = excluded.extra,
            security_score = excluded.security_score,
            security_score_updated_at = excluded.security_score_updated_at,
            restriction_end = excluded.restriction_end,
            coin_balance = excluded.coin_balance,
            violation_count = excluded.violation_count,
            fish_power = excluded.fish_power,
            total_earned = excluded.total_earned,
            fish_shop_enabled = excluded.fish_shop_enabled,
            fish_shop_level = excluded.fish_shop_level,
            seller_30d_sold = excluded.seller_30d_sold,
            owner_username = excluded.owner_username,
            wx_device_id = excluded.wx_device_id
        "#,
    )
    .bind(&snapshot.uid)
    .bind(&snapshot.nick)
    .bind(&snapshot.token)
    .bind(&snapshot.sid)
    .bind(&snapshot.x_umt)
    .bind(&snapshot.cookies)
    .bind(&snapshot.browser_cookie_jar)
    .bind(&snapshot.app_info)
    .bind(&snapshot.status)
    .bind(snapshot.created_at)
    .bind(snapshot.updated_at)
    .bind(snapshot.bound_at)
    .bind(snapshot.cookie_refreshed_at)
    .bind(snapshot.last_session_check_at)
    .bind(snapshot.last_session_ok_at)
    .bind(snapshot.first_session_fail_at)
    .bind(&snapshot.last_session_status)
    .bind(&snapshot.last_session_reason)
    .bind(&snapshot.last_session_evidence)
    .bind(&snapshot.extra)
    .bind(snapshot.security_score)
    .bind(snapshot.security_score_updated_at)
    .bind(&snapshot.restriction_end)
    .bind(snapshot.coin_balance)
    .bind(snapshot.violation_count)
    .bind(snapshot.fish_power)
    .bind(snapshot.total_earned)
    .bind(snapshot.fish_shop_enabled)
    .bind(&snapshot.fish_shop_level)
    .bind(&snapshot.seller_30d_sold)
    .bind(&snapshot.owner_username)
    .bind(&snapshot.wx_device_id)
    .execute(&mut **tx)
    .await
    .map_err(ResponseError::internal)?;
    Ok(())
}

pub(super) async fn local_account_unbind_approved(
    state: &AppState,
    requester_username: &str,
    account_uid: &str,
) -> Result<bool, String> {
    let approved: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT 1
        FROM account_unbind_requests
        WHERE account_uid = ? AND requester_username = ? AND status = 'approved'
        LIMIT 1
        "#,
    )
    .bind(account_uid.trim())
    .bind(requester_username.trim())
    .fetch_optional(state.storage().accounts())
    .await
    .map_err(|error| error.to_string())?;
    Ok(approved.is_some())
}

pub(super) async fn consume_local_account_unbind_result(
    state: &AppState,
    requester_username: &str,
    account_uid: &str,
) -> Result<(), String> {
    let request_id: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM account_unbind_requests
        WHERE account_uid = ? AND requester_username = ? AND status IN ('approved', 'pending')
        ORDER BY CASE status WHEN 'approved' THEN 0 ELSE 1 END, reviewed_at DESC, requested_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(account_uid.trim())
    .bind(requester_username.trim())
    .fetch_optional(state.storage().accounts())
    .await
    .map_err(|error| error.to_string())?;
    let Some(request_id) = request_id else {
        return Ok(());
    };

    let now = unix_seconds_now();
    sqlx::query(
        r#"
        UPDATE account_unbind_requests
        SET status = 'completed',
            updated_at = ?,
            reviewed_by = CASE
                WHEN TRIM(reviewed_by) = '' THEN 'qr-import'
                ELSE reviewed_by
            END,
            reviewed_at = CASE
                WHEN reviewed_at <= 0 THEN ?
                ELSE reviewed_at
            END,
            review_note = CASE
                WHEN TRIM(review_note) = '' THEN '二维码重新导入后已完成账号转移'
                ELSE review_note
            END
        WHERE id = ?
        "#,
    )
    .bind(now)
    .bind(now)
    .bind(request_id)
    .execute(state.storage().accounts())
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

async fn remote_account_unbind_request(
    state: &AppState,
    auth_token: &str,
    payload: Value,
) -> Result<Value, ResponseError> {
    let base = license_server_url(state);
    if base.is_empty() {
        return Err(ResponseError::service_unavailable("鉴权服务地址未配置"));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .tls_info(true)
        .build()
        .map_err(ResponseError::internal)?;
    let url = format!(
        "{base}{}",
        crate::sealed_protocol::sealed_str!("/api/me/account-unbind-requests")
    );
    let response = apply_client_version_headers(client.post(&url))
        .bearer_auth(auth_token)
        .json(&payload)
        .send()
        .await
        .map_err(|error| ResponseError::service_unavailable(format!("鉴权服务不可用：{error}")))?;
    verify_response_pin(
        &response,
        &state.config().xianyu.control_plane_cert_pins,
        "license_service",
        &url,
    )
    .map_err(|error| {
        ResponseError::service_unavailable(format!("鉴权服务证书校验失败：{error}"))
    })?;
    if !response.status().is_success() {
        return Err(license_response_error(response).await);
    }
    response
        .json::<Value>()
        .await
        .map_err(ResponseError::internal)
}

pub(super) async fn remote_account_unbind_approved(
    state: &AppState,
    auth_token: &str,
    account_uid: &str,
) -> Result<bool, ResponseError> {
    let base = license_server_url(state);
    if base.is_empty() {
        return Err(ResponseError::service_unavailable("鉴权服务地址未配置"));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .tls_info(true)
        .build()
        .map_err(ResponseError::internal)?;
    let url = format!(
        "{base}{}",
        crate::sealed_protocol::sealed_str!("/api/me/account-unbind-requests/approved")
    );
    let response = apply_client_version_headers(client.get(&url))
        .bearer_auth(auth_token)
        .query(&[("account_uid", account_uid)])
        .send()
        .await
        .map_err(|error| ResponseError::service_unavailable(format!("鉴权服务不可用：{error}")))?;
    verify_response_pin(
        &response,
        &state.config().xianyu.control_plane_cert_pins,
        "license_service",
        &url,
    )
    .map_err(|error| {
        ResponseError::service_unavailable(format!("鉴权服务证书校验失败：{error}"))
    })?;
    if !response.status().is_success() {
        return Err(license_response_error(response).await);
    }
    let payload = response
        .json::<Value>()
        .await
        .map_err(ResponseError::internal)?;
    Ok(payload
        .get("approved")
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

async fn create_account_unbind_request(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    headers: HeaderMap,
    Json(payload): Json<AccountUnbindRequestPayload>,
) -> ApiResult {
    let requester = account_owner_filter(&state, &identity);
    if requester.trim().is_empty() {
        return Err(ResponseError::bad_request("管理员账号无需申请解绑"));
    }

    let account_uid = payload.account_uid.trim();
    if account_uid.is_empty() {
        return Err(ResponseError::bad_request("account_uid 不能为空"));
    }

    let local_account: Option<(String, String)> =
        sqlx::query_as("SELECT owner_username, nick FROM accounts WHERE uid = ? LIMIT 1")
            .bind(account_uid)
            .fetch_optional(state.storage().accounts())
            .await
            .map_err(ResponseError::internal)?;
    let local_existing_owner = local_account
        .as_ref()
        .map(|(existing_owner, _)| existing_owner.trim().to_string())
        .unwrap_or_default();
    let local_account_nick = local_account
        .as_ref()
        .map(|(_, nick)| nick.trim().to_string())
        .unwrap_or_default();

    let reason = normalize_unbind_text(
        payload.reason.as_deref(),
        "扫码导入时发现账号已绑定到其他用户，请求管理员解绑",
    );
    let auth_token = license_auth_token_from_headers_or_context(&headers)
        .ok_or_else(|| ResponseError::bad_request("未找到远程授权会话，请重新登录后再申请解绑"))?;
    let remote_payload = json!({
        "account_uid": account_uid,
        "account_nick": local_account_nick,
        "existing_owner_username": local_existing_owner,
        "reason": reason,
    });
    let mut result = remote_account_unbind_request(&state, &auth_token, remote_payload).await?;
    let existing_owner = result
        .get("existing_owner_username")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(local_existing_owner.as_str())
        .to_string();
    let mut tx = state
        .storage()
        .accounts()
        .begin()
        .await
        .map_err(ResponseError::internal)?;
    let snapshot = load_account_snapshot(&mut *tx, account_uid)
        .await
        .map_err(ResponseError::internal)?;
    let snapshot_json = snapshot
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(ResponseError::internal)?
        .unwrap_or_else(|| "{}".to_string());
    let request_id = upsert_pending_local_account_unbind_request(
        &mut tx,
        account_uid,
        requester.trim(),
        &existing_owner,
        &reason,
        &snapshot_json,
    )
    .await?;
    tx.commit().await.map_err(ResponseError::internal)?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "account",
            action: "account_unbind_requested",
            operator: requester.trim(),
            trigger_source: "manual",
            uid: account_uid,
            account_nick: "",
            target_type: "account",
            target_id: account_uid,
            target_title: "",
            status: "success",
            error_code: "",
            message: "账号解绑申请已提交，等待管理员审核",
            params: json!({
                "requestId": request_id,
                "requester": requester.trim(),
                "existingOwner": existing_owner,
            }),
            before: snapshot
                .as_ref()
                .map(|snapshot| {
                    json!({
                        "ownerUsername": snapshot.owner_username,
                        "status": snapshot.status,
                    })
                })
                .unwrap_or_else(|| json!({})),
            after: json!({
                "requestStatus": "pending",
            }),
            extra: json!({}),
        },
    )
    .await;

    if let Some(object) = result.as_object_mut() {
        object.insert(
            "message".to_string(),
            json!("解绑申请已提交；管理员通过后需要重新扫码添加"),
        );
    }
    Ok(ResponseJson(result))
}

async fn list_account_unbind_requests(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<AccountUnbindListQuery>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let status = query.status.trim();
    let limit = query.limit.clamp(1, 200);
    let rows: Vec<AccountUnbindRequestRow> = if status.is_empty() {
        sqlx::query_as(
            r#"
            SELECT
                id, account_uid, requester_username, existing_owner_username, status, reason,
                requested_at, updated_at, reviewed_by, reviewed_at, review_note, account_snapshot
            FROM account_unbind_requests
            ORDER BY requested_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(state.storage().accounts())
        .await
        .map_err(ResponseError::internal)?
    } else {
        sqlx::query_as(
            r#"
            SELECT
                id, account_uid, requester_username, existing_owner_username, status, reason,
                requested_at, updated_at, reviewed_by, reviewed_at, review_note, account_snapshot
            FROM account_unbind_requests
            WHERE status = ?
            ORDER BY requested_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(status)
        .bind(limit)
        .fetch_all(state.storage().accounts())
        .await
        .map_err(ResponseError::internal)?
    };

    Ok(ResponseJson(json!({
        "data": rows.iter().map(account_unbind_request_row_to_value).collect::<Vec<_>>(),
        "count": rows.len(),
    })))
}

async fn approve_account_unbind_request(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(request_id): Path<i64>,
    Json(payload): Json<AccountUnbindReviewPayload>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let note = normalize_unbind_text(payload.note.as_deref(), "管理员已通过解绑申请");
    let now = unix_seconds_now();
    let updated = sqlx::query(
        r#"
        UPDATE account_unbind_requests
        SET status = 'approved',
            reviewed_by = ?,
            reviewed_at = ?,
            review_note = ?,
            updated_at = ?
        WHERE id = ? AND status = 'pending'
        "#,
    )
    .bind(identity.username.trim())
    .bind(now)
    .bind(&note)
    .bind(now)
    .bind(request_id)
    .execute(state.storage().accounts())
    .await
    .map_err(ResponseError::internal)?;
    if updated.rows_affected() == 0 {
        return Err(ResponseError::bad_request("解绑申请不存在或已审核"));
    }

    let row: AccountUnbindRequestRow = sqlx::query_as(
        r#"
        SELECT
            id, account_uid, requester_username, existing_owner_username, status, reason,
            requested_at, updated_at, reviewed_by, reviewed_at, review_note, account_snapshot
        FROM account_unbind_requests
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(request_id)
    .fetch_one(state.storage().accounts())
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(account_unbind_request_row_to_value(&row)))
}

async fn reject_account_unbind_request(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(request_id): Path<i64>,
    Json(payload): Json<AccountUnbindReviewPayload>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let note = normalize_unbind_text(payload.note.as_deref(), "管理员已驳回解绑申请");
    let now = unix_seconds_now();
    let mut tx = state
        .storage()
        .accounts()
        .begin()
        .await
        .map_err(ResponseError::internal)?;
    let row: AccountUnbindRequestRow = sqlx::query_as(
        r#"
        SELECT
            id, account_uid, requester_username, existing_owner_username, status, reason,
            requested_at, updated_at, reviewed_by, reviewed_at, review_note, account_snapshot
        FROM account_unbind_requests
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(request_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ResponseError::internal)?
    .ok_or_else(|| ResponseError::not_found("解绑申请不存在"))?;
    if row.status != "pending" {
        return Err(ResponseError::bad_request("解绑申请不存在或已审核"));
    }
    if row.account_snapshot.trim().is_empty() || row.account_snapshot == "{}" {
        return Err(ResponseError::bad_request(
            "解绑申请缺少申请前快照，无法恢复旧账号",
        ));
    }
    let snapshot: AccountSnapshot =
        serde_json::from_str(&row.account_snapshot).map_err(ResponseError::internal)?;
    restore_account_snapshot(&mut tx, &snapshot).await?;
    sqlx::query(
        r#"
        UPDATE account_unbind_requests
        SET status = 'rejected',
            reviewed_by = ?,
            reviewed_at = ?,
            review_note = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(identity.username.trim())
    .bind(now)
    .bind(&note)
    .bind(now)
    .bind(request_id)
    .execute(&mut *tx)
    .await
    .map_err(ResponseError::internal)?;
    tx.commit().await.map_err(ResponseError::internal)?;

    let updated_row: AccountUnbindRequestRow = sqlx::query_as(
        r#"
        SELECT
            id, account_uid, requester_username, existing_owner_username, status, reason,
            requested_at, updated_at, reviewed_by, reviewed_at, review_note, account_snapshot
        FROM account_unbind_requests
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(request_id)
    .fetch_one(state.storage().accounts())
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(account_unbind_request_row_to_value(
        &updated_row,
    )))
}

async fn ensure_account_access(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), ResponseError> {
    let owner = account_owner_filter(state, identity);
    let owner = if owner.is_empty() {
        None
    } else {
        Some(owner.as_str())
    };
    ensure_account_uid_accessible(state.storage().accounts(), owner, uid).await
}

async fn load_location_or_default(state: &AppState, uid: &str) -> LocationContext {
    xianyu_client::load_location(state.storage().accounts(), uid)
        .await
        .unwrap_or_default()
}

fn location_payload(location: &LocationContext) -> Map<String, Value> {
    json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "latitude": location.latitude,
        "longitude": location.longitude,
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}
