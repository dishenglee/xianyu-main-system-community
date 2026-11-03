use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::OnceLock,
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::{
    sync::{mpsc, watch},
    time::{self, MissedTickBehavior},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message as TungsteniteMessage},
};
use tracing::{debug, info, warn};

use crate::{
    logging::env_flag_enabled,
    routes::accounts::{
        decode_and_process_live_sync_payload, do_wx_live_session_keepalive,
        fetch_wx_live_access_token, generate_live_mid, parse_ws_json_frame,
        try_process_fallback_live_event, wx_live_ws_connect_timeout, wx_live_ws_url,
        WX_LIVE_ACCS_APP_KEY, WX_LIVE_ORIGIN, WX_LIVE_USER_AGENT,
    },
    routes::error::ResponseError,
    state::{AckDispatch, AppState, LiveResponseWaiterKind},
    telemetry::IncidentInput,
};

use super::{
    protocol::{
        acknowledge_sync_push, escape_json_string_inline, event_looks_like_trade,
        extract_correlation_group_id, extract_max_pts, is_sync_push_payload, maybe_autosync_orders,
        send_cursor_ack, unix_timestamp_millis,
    },
    session::{
        is_session_dead_error, publish_connection_state, reconnect_delay,
        refresh_cookies_for_handshake,
    },
    COOKIE_REFRESH_COOLDOWN, LIVE_HEARTBEAT_INTERVAL,
};

const LIVE_CONNECTION_STABLE_AFTER: Duration = Duration::from_secs(30);

fn reconnect_jitter(uid: &str, attempt: i64) -> Duration {
    let mut hasher = DefaultHasher::new();
    uid.hash(&mut hasher);
    attempt.hash(&mut hasher);
    Duration::from_millis(hasher.finish() % 1_500)
}

#[derive(Debug, Clone, Default)]
struct LiveDiagnosticCapture {
    matched: bool,
    include_payloads: bool,
    username: String,
    device_id: String,
}

fn truncate_for_log(text: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }

    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => &text[..idx],
        None => text,
    }
}

fn live_debug_logs_enabled() -> bool {
    if !env_flag_enabled("XIANYU_LIVE_DEBUG") {
        return false;
    }
    if std::env::var("APP_RUNTIME_MODE")
        .ok()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("debug"))
    {
        return true;
    }

    static WARN_ONCE: OnceLock<()> = OnceLock::new();
    WARN_ONCE.get_or_init(|| {
        warn!(
            target: "live.worker",
            stream = "runtime",
            module = "live-worker",
            action = "diagnostic_bypass_ignored",
            outcome = "skipped",
            error_code = "diagnostic_bypass_disabled",
            "ignoring XIANYU_LIVE_DEBUG outside debug runtime mode"
        );
    });
    false
}

async fn live_diagnostic_capture(
    state: &AppState,
    module: &str,
    uid: &str,
    session_id: Option<&str>,
    sampling_key: Option<&str>,
) -> LiveDiagnosticCapture {
    let username = sqlx::query_scalar::<_, String>(
        "SELECT owner_username FROM accounts WHERE uid = ? LIMIT 1",
    )
    .bind(uid)
    .fetch_optional(state.storage().accounts())
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let device_id = crate::license_runtime::get_sign_license_context()
        .map(|context| context.device_id)
        .unwrap_or_default();

    if live_debug_logs_enabled() {
        return LiveDiagnosticCapture {
            matched: true,
            include_payloads: true,
            username,
            device_id,
        };
    }

    let decision = state
        .storage()
        .runtime_diagnostic_policy_decision(
            Some(module),
            (!username.trim().is_empty()).then_some(username.as_str()),
            (!device_id.trim().is_empty()).then_some(device_id.as_str()),
            Some(uid),
            session_id.filter(|value| !value.trim().is_empty()),
            None,
            sampling_key.or(Some(uid)),
        )
        .await
        .ok();

    LiveDiagnosticCapture {
        matched: decision
            .as_ref()
            .and_then(|value| value.get("matched"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        include_payloads: decision
            .as_ref()
            .and_then(|value| value.get("includePayloads"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        username,
        device_id,
    }
}

fn looks_like_list_messages_response(payload: &serde_json::Value) -> bool {
    let body = payload.get("body").or_else(|| payload.get("data"));
    let Some(body) = body else {
        return false;
    };

    body.get("messages")
        .and_then(serde_json::Value::as_array)
        .is_some()
        && !body
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .is_empty()
        && body.get("hasMore").is_some()
        && body.get("type").is_some()
}

async fn report_live_worker_incident(
    state: &AppState,
    uid: &str,
    action: &str,
    error_code: &str,
    error_message: String,
    retryable: bool,
    context_json: serde_json::Value,
) {
    let _ = state
        .incident_reporter()
        .report_from_state(
            state,
            IncidentInput {
                error_level: "warn".to_string(),
                module: "live".to_string(),
                action: action.to_string(),
                route: crate::sealed_protocol::sealed_str!("/api/live/worker").to_string(),
                error_code: error_code.to_string(),
                error_message,
                http_status: None,
                upstream_ret: String::new(),
                retryable,
                context_json: json!({
                    "uid": uid,
                    "worker": "live_account",
                    "details": context_json,
                }),
                environment: None,
            },
        )
        .await;
}

pub(super) async fn run_live_account_worker(
    state: AppState,
    uid: String,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut reconnect_attempts = 0_i64;
    let mut last_cookie_refresh: Option<Instant> = None;
    let mut last_pts = 0_i64;
    let mut last_high_pts = 0_i64;

    match state.storage().get_live_sync_cursor(&uid).await {
        Ok((stored_pts, stored_high_pts)) if stored_pts > 0 => {
            last_pts = stored_pts;
            last_high_pts = stored_high_pts.max(0);
        }
        Ok(_) => {}
        Err(error) => {
            warn!(
                target: "live.worker",
                stream = "runtime",
                module = "live-worker",
                action = "restore_cursor",
                outcome = "warn",
                account_uid = %uid,
                error_code = "live_cursor_restore_failed",
                retryable = true,
                detail = %error,
                "failed to restore live cursor"
            );
            report_live_worker_incident(
                &state,
                &uid,
                "restore_cursor",
                "live_cursor_restore_failed",
                error.to_string(),
                true,
                json!({}),
            )
            .await;
        }
    }

    // Python parity (`xianyu_wx_live_client.py:259-287 run_forever`):
    //   while not self._closed:
    //       try:
    //           await self._connect_and_consume()
    //           if self._closed: break
    //           await self._set_state("disconnected", "websocket closed")
    //       except CancelledError: raise
    //       except Exception as exc:
    //           if self._closed: break
    //           await self._set_state("reconnecting", str(exc))
    //       if self._closed: break
    //       await asyncio.wait_for(stop_event.wait(), timeout=next_delay)
    //       self._reconnect_attempts += 1

    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        match connect_and_stream_live_account(
            &state,
            &uid,
            &mut shutdown_rx,
            &mut reconnect_attempts,
            &mut last_pts,
            &mut last_high_pts,
        )
        .await
        {
            Ok(true) => {
                // Clean shutdown (CancelledError equivalent)
                break;
            }
            Ok(false) => {
                // Graceful WS close — Python: _set_state("disconnected", "websocket closed")
                if *shutdown_rx.borrow() {
                    break;
                }
                publish_connection_state(&state, &uid, "disconnected", 0, "websocket closed").await;
            }
            Err(error) => {
                // Exception — Python: _set_state("reconnecting", str(exc))
                if *shutdown_rx.borrow() {
                    break;
                }
                let lower = error.to_ascii_lowercase();
                let noisy = lower.contains("websocket closed")
                    || lower.contains("connection closed")
                    || lower.contains("close frame")
                    || lower.contains("already closed");
                if !noisy {
                    report_live_worker_incident(
                        &state,
                        &uid,
                        "stream_reconnect",
                        "live_stream_error",
                        error.clone(),
                        true,
                        json!({
                            "reconnectAttempts": reconnect_attempts,
                            "lastPts": last_pts,
                            "lastHighPts": last_high_pts,
                        }),
                    )
                    .await;
                }
                publish_connection_state(&state, &uid, "reconnecting", reconnect_attempts, &error)
                    .await;

                // Cookie refresh for dead sessions (Rust-only, no state
                // published — Python handles this inside _fetch_access_token).
                if is_session_dead_error(&error) {
                    let should_refresh = last_cookie_refresh
                        .map(|t| t.elapsed() >= COOKIE_REFRESH_COOLDOWN)
                        .unwrap_or(true);
                    if should_refresh {
                        last_cookie_refresh = Some(Instant::now());
                        let refreshed = refresh_cookies_for_handshake(&state, &uid).await;
                        if refreshed {
                            reconnect_attempts = 0;
                        }
                    }
                }
            }
        }

        if *shutdown_rx.borrow() {
            break;
        }

        // Python: await asyncio.wait_for(stop_event.wait(), timeout=next_delay)
        let delay =
            reconnect_delay(reconnect_attempts) + reconnect_jitter(&uid, reconnect_attempts);
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = time::sleep(delay) => {}
        }
        // Python: self._reconnect_attempts += 1
        reconnect_attempts += 1;
    }

    // Python: close() → _set_state("disconnected", "closed")
    publish_connection_state(&state, &uid, "disconnected", reconnect_attempts, "closed").await;
}

async fn connect_and_stream_live_account(
    state: &AppState,
    uid: &str,
    shutdown_rx: &mut watch::Receiver<bool>,
    reconnect_attempts: &mut i64,
    last_pts: &mut i64,
    last_high_pts: &mut i64,
) -> Result<bool, String> {
    // Returns Ok(true) = clean shutdown, Ok(false) = graceful WS close, Err = error
    let _conn_started = std::time::Instant::now();
    let (access_token, device_id) =
        fetch_wx_live_access_token(state, uid)
            .await
            .map_err(|error| {
                let detail = error.to_string();
                let state_cloned = state.clone();
                let uid_owned = uid.to_string();
                let context = json!({
                    "reconnectAttempts": *reconnect_attempts,
                    "lastPts": *last_pts,
                    "lastHighPts": *last_high_pts,
                });
                let error_message = detail.clone();
                tokio::spawn(async move {
                    report_live_worker_incident(
                        &state_cloned,
                        &uid_owned,
                        "acquire_access_token",
                        "live_access_token_fetch_failed",
                        error_message,
                        true,
                        context,
                    )
                    .await;
                });
                detail
            })?;

    let mut request = wx_live_ws_url()
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request.headers_mut().insert(
        "User-Agent",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static(WX_LIVE_USER_AGENT),
    );
    request.headers_mut().insert(
        "Origin",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static(WX_LIVE_ORIGIN),
    );

    let ws_connect_timeout = wx_live_ws_connect_timeout();
    let (mut websocket, _) = tokio::time::timeout(ws_connect_timeout, connect_async(request))
        .await
        .map_err(|_| {
            format!(
                "wx live websocket connect timeout after {:?}",
                ws_connect_timeout
            )
        })?
        .map_err(|error| {
            let detail = error.to_string();
            let state_cloned = state.clone();
            let uid_owned = uid.to_string();
            let context = json!({
                "reconnectAttempts": *reconnect_attempts,
                "lastPts": *last_pts,
                "lastHighPts": *last_high_pts,
            });
            let error_message = detail.clone();
            tokio::spawn(async move {
                report_live_worker_incident(
                    &state_cloned,
                    &uid_owned,
                    "connect_websocket",
                    "live_websocket_connect_failed",
                    error_message,
                    true,
                    context,
                )
                .await;
            });
            detail
        })?;

    // CRITICAL: serde_json::json!() uses BTreeMap → keys come out alphabetically,
    // which puts `headers` before `lwp` (and reorders headers internally too).
    // The mtop ACCS server validates field order — out-of-order frames are
    // dropped silently, the connection lingers a few seconds collecting
    // fingerprint heartbeats, then the server RSTs the TCP socket. We hand-
    // build the JSON so order matches Python POC `_register_connection`
    // exactly (lwp/headers, then within headers cache-header/app-key/token/ua/
    // dt/wv/sync/did/mid). Same root cause + fix as `send_text`.
    // Python POC sends with json.dumps default separators (", ", ": ") —
    // i.e. spaces after both `,` and `:`. The server's WS frame parser
    // appears to be byte-strict (probably hashing the raw payload for
    // anti-replay), so a tight no-space serialization triggers an ~8s RST.
    // Mirror Python exactly.
    let reg_mid = generate_live_mid();
    let sync_header = if *last_pts > 0 {
        format!("{},{};0;0;", *last_pts, *last_high_pts)
    } else {
        "0,0;0;0;".to_string()
    };
    let reg_payload = format!(
        concat!(
            r#"{{"lwp": "/reg", "headers": {{"#,
            r#""cache-header": "app-key token ua wv", "#,
            r#""app-key": "{appkey}", "#,
            r#""token": "{token}", "#,
            r#""ua": "{ua}", "#,
            r#""dt": "j", "#,
            r#""wv": "im:3,au:3,sy:6", "#,
            r#""sync": "{sync}", "#,
            r#""did": "{did}", "#,
            r#""mid": "{mid}""#,
            r#"}}}}"#,
        ),
        appkey = WX_LIVE_ACCS_APP_KEY,
        token = escape_json_string_inline(&access_token),
        ua = escape_json_string_inline(WX_LIVE_USER_AGENT),
        sync = sync_header,
        did = escape_json_string_inline(&device_id),
        mid = reg_mid,
    );
    websocket
        .send(TungsteniteMessage::Text(reg_payload))
        .await
        .map_err(|error| error.to_string())?;

    // Mirror Python POC `_register_connection`: sleep 1s before sending
    // ackDiff, do NOT block waiting for the /reg ACK. Captured Python frames
    // show ackDiff sent ~1s after /reg, with reg-ack arriving 1ms later.
    // Rust previously waited for /reg ack (typically ~50ms) then sent ackDiff
    // immediately, putting the ackDiff ~950ms earlier than the server
    // expects. The earlier ackDiff is silently treated as out-of-order and
    // the server RSTs the socket ~8s later. Reverting to the Python timing
    // makes the server treat us as a well-behaved client.
    tokio::time::sleep(Duration::from_secs(1)).await;
    // (The previous `loop { tokio::time::timeout_at(reg_ack_deadline, ws.next()) }`
    // block has been removed in favour of the unconditional 1s sleep above —
    // matches Python POC `_register_connection` behaviour and avoids the
    // server-side ~8s RST that triggered when our ackDiff arrived too soon
    // after /reg. Auth failures surface via the select! loop below as
    // websocket errors that `is_auth_error` already classifies for cookie
    // refresh.)

    let init_ack_pts = ((unix_timestamp_millis() as i64) * 1000).max(*last_pts);
    send_cursor_ack(&mut websocket, init_ack_pts, 0)
        .await
        .map_err(|error| error.to_string())?;

    let connected_at = Instant::now();
    let mut stable_connection_recorded = false;

    // Register an outbound-payload channel BEFORE publishing "connected" so
    // any HTTP send handler racing on the state transition sees a ready
    // sender the moment the UI / other client observes "connected".
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register(uid.to_string(), out_tx.clone())
        .await;

    publish_connection_state(state, uid, "connected", 0, "").await;
    // Guard so early returns (error paths) also clean up.
    //
    // IMPORTANT: use `unregister_if_matches` with our own `out_tx` identity.
    // When this socket drops → reconnects quickly, Drop's spawned unregister
    // can race *after* the new worker's `register`, wiping the fresh entry
    // and causing HTTP sends to 503. Comparing sender identity makes Drop
    // a no-op when someone else has already replaced us.
    struct SenderGuard<'a> {
        state: &'a AppState,
        uid: &'a str,
        tx: mpsc::UnboundedSender<String>,
    }
    impl Drop for SenderGuard<'_> {
        fn drop(&mut self) {
            // Fast path: try to unregister synchronously. The lock is
            // typically uncontended (one writer at a time across all
            // workers + HTTP handlers' brief read locks), so this succeeds
            // ~all the time and removes the stale entry IMMEDIATELY when
            // the worker function returns. Without sync removal an HTTP
            // handler could read the still-registered (but channel-closed)
            // sender between the worker returning and the spawned async
            // unregister scheduling.
            //
            // We still clear the closed-channel via `get_alive` on the
            // HTTP side, so this is just an optimization to avoid the
            // first 503 when reconnects are fast. It's not load-bearing
            // for correctness.
            if self
                .state
                .live_senders()
                .try_unregister_if_matches_sync(self.uid, &self.tx)
            {
                return;
            }
            // Fall back to async spawn if the write lock was contended.
            // Keep `unregister_if_matches` (not bare unregister) so we
            // don't wipe a fresh sender registered by the next reconnect.
            let senders = self.state.live_senders().clone();
            let uid = self.uid.to_string();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                senders.unregister_if_matches(&uid, &tx).await;
            });
        }
    }
    let _sender_guard = SenderGuard {
        state,
        uid,
        tx: out_tx,
    };

    // Python POC `_heartbeat_loop`: first tick at 10s, subsequent ticks every
    // 15s. tokio::time::interval would fire its first tick immediately inside
    // a select!, which (in conjunction with cursor-ack on heartbeat below)
    // sends a /! frame ~0ms after our ackDiff — the server seems to interpret
    // that as a misbehaving client and RSTs the socket ~8s later. Use
    // `interval_at(now+10s, 15s)` to match Python timing.
    let mut heartbeat = time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(10),
        LIVE_HEARTBEAT_INTERVAL,
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Python parity: 两个后台周期任务都嵌在 select! 里:
    //   * `token_check`   — 每 60s 检查 access_token 是否临近 20h 过期，
    //     过期就 fetch 新 token + 重新 /reg（同一 socket 上复用，避免断连）
    //   * `session_keepalive` — 每 10 分钟调一次轻量 MTOP 保活 cookies
    //     (mtop.taobao.idlemessage.user.query)，遇到 _m_h5_tk 过期自动用新 c 回写
    let token_acquired = Instant::now();
    // 两个后台任务都让 first tick **不立刻触发**，否则 tokio interval 会在
    // 第一轮立刻跑，抢占刚 /reg 完的 WS 造成消息丢失。interval_at 从 now+period
    // 开始。
    let mut token_check = time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(60),
        Duration::from_secs(60),
    );
    token_check.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut session_keepalive = time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(600),
        Duration::from_secs(600),
    );
    session_keepalive.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    let _ = websocket.close(None).await;
                    return Ok(true);
                }
            }
            _ = heartbeat.tick() => {
                // Python parity: `_heartbeat_loop` (`xianyu_wx_live_client.py:483-493`)
                // sends ONLY `/!` — no piggybacked cursor-ack, no body. The
                // previous Rust impl appended an `ackDiff` with `pts={last_pts}`
                // on every heartbeat, but the server's `pts` field is encoded
                // in microseconds (`ms*1000`, see `init_ack` payload), and a
                // real `last_pts` from `syncPushPackage.maxPts` is in the
                // 1.7e15 range — over JS Number.MAX_SAFE_INTEGER (2^53 ≈ 9e15
                // wait that's actually OK, but in practice goofish parses
                // these via something that mis-handles the magnitude). The
                // upshot: every ~6 heartbeats (90-100s), the server RST'd the
                // TCP socket with "Connection reset without closing
                // handshake". Removing the piggyback eliminates the
                // periodic disconnect cycle.
                //
                // (`acknowledge_sync_push` in the frame branch handles the
                // per-frame ACK that Python's `_send_ack` does — that's the
                // mechanism keeping us in "live push" mode. The piggyback
                // was redundant and harmful.)
                let hb = format!(
                    r#"{{"lwp": "/!", "headers": {{"mid": "{mid}"}}}}"#,
                    mid = generate_live_mid(),
                );
                if let Err(error) = websocket.send(TungsteniteMessage::Text(hb)).await {
                    warn!(
                        target: "live.worker",
                        stream = "runtime",
                        module = "live-worker",
                        action = "heartbeat_ping",
                        outcome = "warn",
                        account_uid = %uid,
                        error_code = "live_heartbeat_send_failed",
                        retryable = true,
                        detail = %error,
                        "failed to send live heartbeat"
                    );
                    report_live_worker_incident(
                        state,
                        uid,
                        "heartbeat_ping",
                        "live_heartbeat_send_failed",
                        error.to_string(),
                        true,
                        json!({
                            "reconnectAttempts": *reconnect_attempts,
                            "lastPts": *last_pts,
                            "lastHighPts": *last_high_pts,
                        }),
                    )
                    .await;
                    return Err(error.to_string());
                }
                if !stable_connection_recorded
                    && connected_at.elapsed() >= LIVE_CONNECTION_STABLE_AFTER
                {
                    stable_connection_recorded = true;
                    *reconnect_attempts = 0;
                }
            }
            _ = token_check.tick() => {
                // token 有效期 24h，20h 提前续期。第一次 tick 会立刻触发，
                // 跳过未到期的。
                if token_acquired.elapsed() < Duration::from_secs(20 * 3600) {
                    continue;
                }
                info!(
                    target: "live.worker",
                    stream = "runtime",
                    module = "live-worker",
                    action = "token_refresh_started",
                    outcome = "info",
                    account_uid = %uid,
                    "starting live access token refresh"
                );
                match fetch_wx_live_access_token(state, uid).await {
                    Ok((new_token, did)) => {
                        info!(
                            target: "live.worker",
                            stream = "runtime",
                            module = "live-worker",
                            action = "token_refresh_reg",
                            outcome = "info",
                            account_uid = %uid,
                            "refreshed live access token, re-registering socket"
                        );
                        // Python parity (`xianyu_wx_live_client.py:519-521`):
                        // token 续期后调 `_register_connection(websocket, new_token)`,
                        // 即重发 /reg 然后 1s 后重发 /r/SyncStatus/ackDiff。
                        // Rust 之前只发 /reg, 服务端会重置 sync 状态需要再次
                        // ackDiff 才能恢复 live push 模式 — 漏发 ackDiff 会
                        // 让 token 续期那 20h 一次的窗口期消息丢失。
                        // 同时手拼 JSON 必须**带空格** (`, ` 和 `: `) 与初始
                        // /reg 完全一致 (服务端 byte-strict; 同 L737 注释).
                        let sync_header = if *last_pts > 0 {
                            format!("{},{};0;0;", *last_pts, *last_high_pts)
                        } else {
                            "0,0;0;0;".to_string()
                        };
                        let reg_payload = format!(
                            concat!(
                                r#"{{"lwp": "/reg", "headers": {{"#,
                                r#""cache-header": "app-key token ua wv", "#,
                                r#""app-key": "{appkey}", "#,
                                r#""token": "{token}", "#,
                                r#""ua": "{ua}", "#,
                                r#""dt": "j", "#,
                                r#""wv": "im:3,au:3,sy:6", "#,
                                r#""sync": "{sync}", "#,
                                r#""did": "{did}", "#,
                                r#""mid": "{mid}""#,
                                r#"}}}}"#,
                            ),
                            appkey = WX_LIVE_ACCS_APP_KEY,
                            token = escape_json_string_inline(&new_token),
                            ua = escape_json_string_inline(WX_LIVE_USER_AGENT),
                            sync = sync_header,
                            did = escape_json_string_inline(&did),
                            mid = generate_live_mid(),
                        );
                        if let Err(e) = websocket.send(TungsteniteMessage::Text(reg_payload)).await {
                            warn!(
                                target: "live.worker",
                                stream = "runtime",
                                module = "live-worker",
                                action = "token_refresh_reg",
                                outcome = "warn",
                                account_uid = %uid,
                                error_code = "live_reregister_failed",
                                retryable = true,
                                detail = %e,
                                "failed to re-register live socket after token refresh"
                            );
                        } else {
                            // F-1 fix: 等 1s 然后发 ackDiff (同 _register_connection)
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            let ack_pts = ((unix_timestamp_millis() as i64) * 1000).max(*last_pts);
                            if let Err(e) = send_cursor_ack(&mut websocket, ack_pts, *last_high_pts).await {
                                warn!(
                                    target: "live.worker",
                                    stream = "runtime",
                                    module = "live-worker",
                                    action = "token_refresh_ack_diff",
                                    outcome = "warn",
                                    account_uid = %uid,
                                    error_code = "live_ack_diff_failed",
                                    retryable = true,
                                    detail = %e,
                                    "failed to send ackDiff after token refresh"
                                );
                                report_live_worker_incident(
                                    state,
                                    uid,
                                    "token_refresh_ack_diff",
                                    "live_ack_diff_failed",
                                    e.to_string(),
                                    true,
                                    json!({
                                        "stage": "token_refresh",
                                        "lastPts": *last_pts,
                                        "lastHighPts": *last_high_pts,
                                    }),
                                )
                                .await;
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            target: "live.worker",
                            stream = "runtime",
                            module = "live-worker",
                            action = "token_refresh",
                            outcome = "warn",
                            account_uid = %uid,
                            error_code = "live_access_token_refresh_failed",
                            retryable = true,
                            detail = %e,
                            "failed to refresh live access token"
                        );
                        report_live_worker_incident(
                            state,
                            uid,
                            "token_refresh",
                            "live_access_token_refresh_failed",
                            e.to_string(),
                            true,
                            json!({
                                "stage": "token_refresh",
                                "reconnectAttempts": *reconnect_attempts,
                                "lastPts": *last_pts,
                                "lastHighPts": *last_high_pts,
                            }),
                        )
                        .await;
                    }
                }
            }
            _ = session_keepalive.tick() => {
                // Python parity `_do_session_keepalive` (`xianyu_wx_live_client.py:540-565`):
                // call `mtop.taobao.idlemessage.user.query` (lightweight, no
                // appKey/deviceId in body). If the response carries a fresh
                // `c`, persist new `_m_h5_tk`/`_m_h5_tk_enc` to DB so next
                // process restart skips the round-1 401 re-handshake.
                //
                // The previous Rust impl re-fetched `mtop.wx.login.token`
                // every 10 minutes, which is itself a risk-control signal —
                // a healthy client should rarely re-fetch its accessToken.
                if let Err(e) = do_wx_live_session_keepalive(state, uid).await {
                    warn!(
                        target: "live.worker",
                        stream = "runtime",
                        module = "live-worker",
                        action = "session_keepalive",
                        outcome = "warn",
                        account_uid = %uid,
                        error_code = "live_session_keepalive_failed",
                        retryable = true,
                        detail = %e,
                        "live session keepalive failed"
                    );
                    report_live_worker_incident(
                        state,
                        uid,
                        "session_keepalive",
                        "live_session_keepalive_failed",
                        e.to_string(),
                        true,
                        json!({
                            "reconnectAttempts": *reconnect_attempts,
                            "lastPts": *last_pts,
                            "lastHighPts": *last_high_pts,
                        }),
                    )
                    .await;
                }
            }
            outbound = out_rx.recv() => {
                let Some(payload) = outbound else { continue };
                let outbound_diagnostic =
                    live_diagnostic_capture(&state, "live-worker", uid, None, Some(uid)).await;
                if outbound_diagnostic.matched {
                    if outbound_diagnostic.include_payloads {
                        info!(
                            target: "live.worker",
                            stream = "runtime",
                            module = "live-worker",
                            action = "outbound_frame",
                            username = %outbound_diagnostic.username,
                            device_id = %outbound_diagnostic.device_id,
                            account_uid = %uid,
                            payload_bytes = payload.len(),
                            payload_preview = %truncate_for_log(&payload, 160),
                            "sending live outbound frame"
                        );
                    } else {
                        info!(
                            target: "live.worker",
                            stream = "runtime",
                            module = "live-worker",
                            action = "outbound_frame",
                            username = %outbound_diagnostic.username,
                            device_id = %outbound_diagnostic.device_id,
                            account_uid = %uid,
                            payload_bytes = payload.len(),
                            "sending live outbound frame"
                        );
                    }
                }
                if let Err(e) = websocket.send(TungsteniteMessage::Text(payload)).await {
                    warn!(
                        target: "live.worker",
                        stream = "runtime",
                        module = "live-worker",
                        action = "outbound_send",
                        outcome = "warn",
                        account_uid = %uid,
                        error_code = "live_outbound_send_failed",
                        retryable = true,
                        detail = %e,
                        "failed to send live outbound frame"
                    );
                    return Err(e.to_string());
                }
                if outbound_diagnostic.matched {
                    info!(
                        target: "live.worker",
                        stream = "runtime",
                        module = "live-worker",
                        action = "outbound_send",
                        outcome = "success",
                        username = %outbound_diagnostic.username,
                        device_id = %outbound_diagnostic.device_id,
                        account_uid = %uid,
                        "sent live outbound frame"
                    );
                }
            }
            frame = websocket.next() => {
                let Some(frame) = frame else {
                    warn!(
                        target: "live.worker",
                        stream = "runtime",
                        module = "live-worker",
                        action = "websocket_next",
                        outcome = "warn",
                        account_uid = %uid,
                        error_code = "live_websocket_closed",
                        retryable = true,
                        "live websocket stream ended"
                    );
                    return Ok(false);
                };
                let frame = match frame {
                    Ok(f) => f,
                    Err(error) => {
                        warn!(
                            target: "live.worker",
                            stream = "runtime",
                            module = "live-worker",
                            action = "websocket_next",
                            outcome = "warn",
                            account_uid = %uid,
                            error_code = "live_websocket_next_failed",
                            retryable = true,
                            detail = %error,
                            "live websocket read failed"
                        );
                        return Err(error.to_string());
                    }
                };
                let Some(payload) = parse_ws_json_frame(frame).map_err(|error: ResponseError| error.to_string())? else {
                    continue;
                };
                if !stable_connection_recorded
                    && connected_at.elapsed() >= LIVE_CONNECTION_STABLE_AFTER
                {
                    stable_connection_recorded = true;
                    *reconnect_attempts = 0;
                }
                // Python parity (`xianyu_wx_live_client.py:608-612`): wake any
                // pending RPC waiter whose `mid` matches this frame's headers.
                // This is how `create_chat`, `list_messages`, and `send_image`
                // (HTTP handlers running on a different task) get their
                // synchronous response. We do this BEFORE `_send_ack` /
                // `process_sync_push` so the waiter sees the raw payload.
                // The take is non-blocking: if no waiter is registered, this
                // is just a HashMap lookup miss.
                let mut matched_waiter_kind = None;
                {
                    let mid_in_frame = payload
                        .get("headers")
                        .and_then(|h| h.get("mid"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !mid_in_frame.is_empty() {
                        if mid_in_frame == reg_mid {
                            if let Some(cgi) = extract_correlation_group_id(&payload) {
                                info!(
                                    target: "live.worker",
                                    stream = "runtime",
                                    module = "live-worker",
                                    action = "register_correlation_group",
                                    outcome = "success",
                                    account_uid = %uid,
                                    correlation_group_id = %cgi,
                                    "captured live correlation group id"
                                );
                                if let Err(e) = state.storage().update_correlation_group_id(uid, &cgi).await {
                                    warn!(
                                        target: "live.worker",
                                        stream = "runtime",
                                        module = "live-worker",
                                        action = "persist_correlation_group",
                                        outcome = "warn",
                                        account_uid = %uid,
                                        error_code = "persist_correlation_group_failed",
                                        retryable = true,
                                        detail = %e,
                                        "failed to persist live correlation group id"
                                    );
                                }
                            }
                        }
                        let waiter_kind = state.live_response_waiters().kind(uid, &mid_in_frame).await;
                        match state
                            .live_response_waiters()
                            .dispatch_if_matches(uid, &mid_in_frame, &payload)
                            .await
                        {
                            AckDispatch::Delivered => {
                                matched_waiter_kind = waiter_kind.clone();
                            }
                            AckDispatch::Rejected => warn!(
                                target: "live.worker",
                                stream = "runtime",
                                module = "live-worker",
                                action = "send_ack_rejected",
                                outcome = "skipped",
                                account_uid = %uid,
                                error_code = "send_ack_mismatch",
                                "ignored mismatched send ACK while preserving waiter"
                            ),
                            AckDispatch::Missing => {}
                        }
                    }
                }
                // Debug: peek at non-sync inbound frames (server ACKs, etc.)
                // so we can see whether /r/MessageSend was accepted. Capped
                // so it doesn't flood the log under heavy sync traffic.
                if !is_sync_push_payload(&payload) {
                    let inbound_diagnostic =
                        live_diagnostic_capture(&state, "live-worker", uid, None, Some(uid)).await;
                    if inbound_diagnostic.matched {
                    let preview = serde_json::to_string(&payload)
                        .unwrap_or_default();
                        if inbound_diagnostic.include_payloads {
                            info!(
                                target: "live.worker",
                                stream = "runtime",
                                module = "live-worker",
                                action = "inbound_frame_preview",
                                username = %inbound_diagnostic.username,
                                device_id = %inbound_diagnostic.device_id,
                                account_uid = %uid,
                                payload_bytes = preview.len(),
                                payload_preview = %truncate_for_log(&preview, 1500),
                                "captured live inbound frame preview"
                            );
                        } else {
                            info!(
                                target: "live.worker",
                                stream = "runtime",
                                module = "live-worker",
                                action = "inbound_frame_preview",
                                username = %inbound_diagnostic.username,
                                device_id = %inbound_diagnostic.device_id,
                                account_uid = %uid,
                                payload_bytes = preview.len(),
                                "captured live inbound frame metadata"
                            );
                        }
                    }
                }
                // Python parity (`xianyu_wx_live_client.py:614-622
                // _handle_packet` + `:639-687 _handle_sync_package`):
                //   1. Only frames with `body.syncPushPackage.data` non-empty
                //      get ACKed (mid+sid only) and decoded.
                //   2. Empty fingerprint frames (`/s/sync` body =
                //      syncExtensionModel only, or `/s/vulcan` with empty
                //      data) are NOT ACKed — Python's `_handle_packet`
                //      simply returns after `code==200` check or falls
                //      through to fallback parser.
                //   3. The ONLY ackDiff is the initial one in
                //      `_register_connection`; never per-frame.
                if is_sync_push_payload(&payload) {
                    acknowledge_sync_push(&mut websocket, &payload)
                        .await
                        .map_err(|error| error.to_string())?;
                    let (max_pts, max_high_pts) = extract_max_pts(&payload);
                    if max_pts > 0 {
                        if max_pts > *last_pts {
                            *last_pts = max_pts;
                            *last_high_pts = max_high_pts;
                            if let Err(error) = state
                                .storage()
                                .update_live_sync_cursor(uid, *last_pts, *last_high_pts)
                                .await
                            {
                                warn!(
                                    target: "live.worker",
                                    stream = "runtime",
                                    module = "live-worker",
                                    action = "persist_cursor",
                                    outcome = "warn",
                                    account_uid = %uid,
                                    error_code = "live_cursor_persist_failed",
                                    retryable = true,
                                    detail = %error,
                                    "failed to persist live cursor"
                                );
                            }
                        }
                    }
                    let events = decode_and_process_live_sync_payload(state, uid, &payload)
                        .await
                        .map_err(|error| error.to_string())?;
                    if !events.is_empty() {
                        debug!(
                            target: "live.worker",
                            stream = "runtime",
                            module = "live-worker",
                            action = "sync_push_processed",
                            outcome = "success",
                            account_uid = %uid,
                            event_count = events.len(),
                            "processed live sync push payload"
                        );
                    }
                    for event in events {
                        if event_looks_like_trade(&event) {
                            maybe_autosync_orders(state.clone(), uid.to_string());
                        }
                        state.live_events().publish(event);
                    }
                } else {
                    if matched_waiter_kind == Some(LiveResponseWaiterKind::ListMessages) {
                        // `listUserMessages` responses are history backfill,
                        // not fresh live pushes. Their body often contains
                        // `userMessageModels`, which the generic fallback
                        // parser would otherwise reinterpret as a brand-new
                        // inbound message and fan out to retained notification channels.
                        // That produces false "新客户消息" notifications for
                        // stale history resurrected by sessionArouse.
                        continue;
                    }
                    if matched_waiter_kind.is_some() {
                        // Matched RPC replies (send ACKs, create-chat replies,
                        // read/recall ACKs, etc.) belong to their caller and
                        // are not fresh live pushes. Feeding them through the
                        // fallback parser turns ACK payload fragments into
                        // malformed message events without sessionId.
                        continue;
                    }
                    if looks_like_list_messages_response(&payload) {
                        if let Some(waiter) = state
                            .live_response_waiters()
                            .take_first_of_kind_for_uid(uid, LiveResponseWaiterKind::ListMessages)
                            .await
                        {
                            let _ = waiter.tx.send(payload.clone());
                        }
                        // Same as the matched-mid case above: this is history
                        // backfill and must not be reinterpreted as a fresh
                        // inbound live message.
                        continue;
                    }
                    // Python parity (`xianyu_wx_live_client.py:624-634`): non
                    // sync-push frames may still carry message events (e.g. an
                    // RPC response to /r/IMessageManager/createChat that ships
                    // the freshly-created chat's first message inline, system
                    // notifications wrapped as RPC envelopes). Try the
                    // generic fallback parser before dropping the frame —
                    // it recursively unwraps body/data/message/payload keys
                    // and runs the same persist + enrich + publish chain as
                    // sync-push events. Returns None silently on frames that
                    // truly aren't message-shaped (heartbeat ACK echoes, etc.).
                    match try_process_fallback_live_event(state, uid, &payload).await {
                        Ok(events) if !events.is_empty() => {
                            debug!(
                                target: "live.worker",
                                stream = "runtime",
                                module = "live-worker",
                                action = "fallback_event_processed",
                                outcome = "success",
                                account_uid = %uid,
                                event_count = events.len(),
                                "processed fallback live events"
                            );
                            for event in events {
                                if event_looks_like_trade(&event) {
                                    maybe_autosync_orders(state.clone(), uid.to_string());
                                }
                                state.live_events().publish(event);
                            }
                        }
                        Ok(_) => { /* not message-shaped; debug log already printed above */ }
                        Err(e) => {
                            warn!(
                                target: "live.worker",
                                stream = "runtime",
                                module = "live-worker",
                                action = "fallback_parse",
                                outcome = "warn",
                                account_uid = %uid,
                                error_code = "live_fallback_parse_failed",
                                retryable = true,
                                detail = %e,
                                "failed to parse fallback live event"
                            );
                            report_live_worker_incident(
                                state,
                                uid,
                                "fallback_parse",
                                "live_fallback_parse_failed",
                                e.to_string(),
                                true,
                                json!({
                                    "lwp": payload.get("lwp").and_then(serde_json::Value::as_str).unwrap_or(""),
                                    "mid": payload
                                        .get("headers")
                                        .and_then(|headers| headers.get("mid"))
                                        .and_then(serde_json::Value::as_str)
                                        .unwrap_or(""),
                                }),
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_for_log;

    #[test]
    fn truncate_for_log_preserves_utf8_boundaries() {
        let text = "abc中文def";
        assert_eq!(truncate_for_log(text, 0), "");
        assert_eq!(truncate_for_log(text, 1), "a");
        assert_eq!(truncate_for_log(text, 3), "abc");
        assert_eq!(truncate_for_log(text, 4), "abc中");
        assert_eq!(truncate_for_log(text, 5), "abc中文");
        assert_eq!(truncate_for_log(text, 99), text);
    }
}
