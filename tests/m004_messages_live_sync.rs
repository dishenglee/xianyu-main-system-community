use std::{
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn cached_sessions_prefer_real_activity_over_h5_sort_index() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;

    seed_account(&accounts_pool, "uid-live").await;
    seed_live_state_row(
        &live_state_pool,
        "uid-live",
        "sess-live",
        1_776_343_650_001,
        1_776_343_650_001,
        "真正更新更近",
    )
    .await;
    seed_live_state_row(
        &live_state_pool,
        "uid-live",
        "sess-h5",
        20_260_411_080_330,
        1_775_868_126_355,
        "H5 排序值更大但活动更旧",
    )
    .await;

    let payload = request_json(
        app,
        "/api/messages/pc/sessions?uid=uid-live&cached=true&num=10",
    )
    .await;
    let sessions = payload["normalized"]["sessions"].as_array().unwrap();
    assert_eq!(sessions[0]["sessionId"], "sess-live");
    assert_eq!(sessions[1]["sessionId"], "sess-h5");

    clear_env();
}

#[tokio::test]
async fn force_refresh_keeps_live_state_sessions_without_short_ws_sync() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream_state = UpstreamState::default();
    let (base_url, shutdown_http) = spawn_upstream(upstream_state).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", base_url.clone());

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    seed_account(&accounts_pool, "uid-live").await;
    seed_live_state_identity_row(
        &live_state_pool,
        "uid-live",
        "ws-session-1",
        1_714_440_001_000,
        1_714_440_001_000,
        "实时买家",
        "buyer-live",
        "实时商品",
    )
    .await;

    let payload = request_json(
        app,
        "/api/messages/pc/sessions?uid=uid-live&num=20&force=true",
    )
    .await;
    let sessions = payload["normalized"]["sessions"].as_array().unwrap();
    assert_eq!(sessions[0]["sessionId"], "ws-session-1");
    assert_eq!(sessions[0]["peerNick"], "实时买家");
    assert_eq!(sessions[0]["itemTitle"], "实时商品");

    let row = sqlx::query(
        "SELECT latest_version, peer_nick, summary FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("uid-live")
    .bind("ws-session-1")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(row.get::<i64, _>("latest_version"), 1_714_440_001_000);
    assert_eq!(row.get::<String, _>("peer_nick"), "实时买家");

    let _ = shutdown_http.send(());
    clear_env();
}

#[tokio::test]
async fn force_refresh_resolves_placeholder_peer_nick_for_live_only_session() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream_state = UpstreamState::default();
    let (base_url, shutdown_http) = spawn_upstream(upstream_state).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", base_url.clone());

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;

    seed_account(&accounts_pool, "uid-live").await;
    seed_live_state_identity_row(
        &live_state_pool,
        "uid-live",
        "sess-name",
        1_714_440_003_000,
        1_714_440_003_000,
        "用户_123456",
        "peer-1",
        "",
    )
    .await;

    let payload = request_json(
        app,
        "/api/messages/pc/sessions?uid=uid-live&num=20&force=true",
    )
    .await;
    let sessions = payload["normalized"]["sessions"].as_array().unwrap();
    let session = sessions
        .iter()
        .find(|session| session["sessionId"] == "sess-name")
        .unwrap();
    assert_eq!(session["peerNick"], "真实买家昵称");
    assert_eq!(session["peerUid"], "peer-1");
    assert_eq!(session["itemTitle"], "演示商品");

    let row = sqlx::query(
        "SELECT peer_nick, item_title FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("uid-live")
    .bind("sess-name")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("peer_nick"), "真实买家昵称");
    assert_eq!(row.get::<String, _>("item_title"), "演示商品");

    let _ = shutdown_http.send(());
    clear_env();
}

#[tokio::test]
async fn cached_sessions_respect_requested_limit_above_one_hundred() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;

    seed_account(&accounts_pool, "uid-many").await;
    for index in 0..150_i64 {
        seed_live_state_identity_row(
            &live_state_pool,
            "uid-many",
            &format!("sess-{index:03}"),
            1_777_000_000_000_i64 + index,
            1_777_000_000_000_i64 + index,
            &format!("买家{index:03}"),
            &format!("peer-{index:03}"),
            &format!("商品{index:03}"),
        )
        .await;
    }

    let payload = request_json(
        app,
        "/api/messages/pc/sessions?uid=uid-many&cached=true&num=150",
    )
    .await;
    let sessions = payload["normalized"]["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 150);
    assert_eq!(sessions[0]["sessionId"], "sess-149");
    assert_eq!(sessions[149]["sessionId"], "sess-000");

    clear_env();
}

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<String>>>,
}

async fn spawn_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler))
        .route("/h5/:api/:version/:minor/", post(h5_handler_with_minor))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{}", addr), shutdown_tx)
}

async fn h5_handler(
    State(state): State<UpstreamState>,
    Path((api, version)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    h5_response(state, api, version, None, body).await
}

async fn h5_handler_with_minor(
    State(state): State<UpstreamState>,
    Path((api, version, minor)): Path<(String, String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    h5_response(state, api, version, Some(minor), body).await
}

async fn h5_response(
    state: UpstreamState,
    api: String,
    version: String,
    minor: Option<String>,
    _body: Bytes,
) -> axum::response::Response {
    let path = match minor {
        Some(minor) => format!("/h5/{api}/{version}/{minor}/"),
        None => format!("/h5/{api}/{version}/"),
    };
    state.requests.lock().unwrap().push(path.clone());

    match path.as_str() {
        "/h5/mtop.taobao.idlemessage.wx.login.token/1.0/2.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "accessToken": "test-access-token"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.wx.idlemessage.session.sync/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessions": []
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.pc.message.sync/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessionId": "sess-name",
                "type": 1,
                "hasMore": false,
                "messages": [
                    {
                        "messageUuid": "msg-name-1",
                        "version": 1714440003000_i64,
                        "timeStamp": 1714440003000_i64,
                        "content": {
                            "contentType": 1,
                            "text": { "text": "最新一条消息" }
                        },
                        "senderInfo": {
                            "userId": "peer-1",
                            "nick": "真实买家昵称",
                            "fishNick": "真实买家昵称",
                            "logo": "https://img.example/peer-1.png"
                        },
                        "sessionInfo": {
                            "sessionId": "sess-name",
                            "sessionType": 1,
                            "users": [
                                {
                                    "userId": "uid-live",
                                    "nick": "卖家自己",
                                    "fishNick": "卖家自己",
                                    "logo": "https://img.example/self.png"
                                },
                                {
                                    "userId": "peer-1",
                                    "nick": "真实买家昵称",
                                    "fishNick": "真实买家昵称",
                                    "logo": "https://img.example/peer-1.png"
                                }
                            ],
                            "attributeMap": {
                                "squadName_peer-1": "演示商品"
                            },
                            "itemInfo": {
                                "itemId": "item-name",
                                "mainPic": "https://img.example/item-name.png"
                            }
                        }
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.user.query/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "userInfo": {
                    "userId": "peer-1",
                    "fishNick": "真实买家昵称",
                    "logo": "https://img.example/peer-1.png"
                }
            }
        }))
        .into_response(),
        _ => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn request_json(app: Router, uri: &str) -> Value {
    let response = app
        .oneshot(
            Request::get(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

async fn seed_account(pool: &SqlitePool, uid: &str) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid,
            owner_username,
            nick,
            token,
            sid,
            x_umt,
            cookies,
            browser_cookie_jar,
            app_info,
            status,
            created_at,
            updated_at,
            cookie_refreshed_at,
            last_session_check_at,
            last_session_ok_at,
            first_session_fail_at,
            last_session_status,
            last_session_reason,
            last_session_evidence,
            extra,
            security_score,
            security_score_updated_at,
            restriction_end,
            coin_balance,
            violation_count,
            fish_power,
            seller_30d_sold
        )
        VALUES (?, ?, ?, ?, ?, '', ?, '[]', '', 'active', 0, 0, 0, 0, 0, 0, '', '', '', '{}', -1, 0, '', 0, 0, 0, '')
        "#,
    )
    .bind(uid)
    .bind("admin")
    .bind("Seller Live")
    .bind("seed-token_1700000000000")
    .bind("seed-cookie2")
    .bind(
        json!({
            "_m_h5_tk": "seed-token_1700000000000",
            "_m_h5_tk_enc": "seed-enc",
            "cookie2": "seed-cookie2",
            "unb": uid,
            "tracknick": "Seller Live",
            "sgcookie": "seed-sgcookie",
            "csg": "seed-csg"
        })
        .to_string(),
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_live_state_row(
    pool: &SqlitePool,
    uid: &str,
    session_id: &str,
    latest_version: i64,
    last_message_ts: i64,
    summary: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id, unread_count,
            peer_uid, peer_nick, peer_role, peer_logo, self_role, item_id, item_title, item_pic,
            summary, last_message_ts, last_sync_at, last_ws_event_at, last_http_compensate_at, updated_at
        )
        VALUES (?, ?, 1, ?, '', 0, '', '', '', '', '', '', '', '', ?, ?, 0, 0, 0, 0)
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(latest_version)
    .bind(summary)
    .bind(last_message_ts)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_live_state_identity_row(
    pool: &SqlitePool,
    uid: &str,
    session_id: &str,
    latest_version: i64,
    last_message_ts: i64,
    peer_nick: &str,
    peer_uid: &str,
    item_title: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id, unread_count,
            peer_uid, peer_nick, peer_role, peer_logo, self_role, item_id, item_title, item_pic,
            summary, last_message_ts, last_sync_at, last_ws_event_at, last_http_compensate_at, updated_at
        )
        VALUES (?, ?, 1, ?, '', 0, ?, ?, '', '', '', '', ?, '', '最新一条消息', ?, 0, 0, 0, 0)
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(latest_version)
    .bind(peer_uid)
    .bind(peer_nick)
    .bind(item_title)
    .bind(last_message_ts)
    .execute(pool)
    .await
    .unwrap();
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::set_var("XIANYU_LOCAL_ADMIN_LOGIN_ENABLED", "true");
}

fn clear_env() {
    env::remove_var("APP_DATA_DIR");
    env::remove_var("JWT_SECRET");
    env::remove_var("ADMIN_USERNAME");
    env::remove_var("ADMIN_PASSWORD");
    env::remove_var("XIANYU_LOCAL_ADMIN_LOGIN_ENABLED");
    env::remove_var("XIANYU_H5_GATEWAY");
    env::remove_var("XIANYU_WX_GATEWAY");
    env::remove_var("XIANYU_WX_WS_URL");
}
