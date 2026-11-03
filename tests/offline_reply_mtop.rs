use std::{
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app,
    config::AppConfig,
    license_runtime::{set_sign_license_context, SignLicenseContext},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "offline-reply-mtop";

#[derive(Clone, Default)]
struct CapturedRequests(Arc<Mutex<Vec<CapturedRequest>>>);

#[derive(Clone, Debug)]
struct CapturedRequest {
    path: String,
    body: String,
}

#[tokio::test]
async fn offline_reply_sync_uses_app_mtop_for_offline_reply() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    let captured = CapturedRequests::default();
    let (base_url, shutdown_tx) = spawn_upstream(captured.clone()).await;
    configure_app_transport_env(&base_url);
    set_sign_license_context(Some(test_sign_license_context(&base_url)));

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts()).await;
    let app = build_app(state);

    let response = app
        .oneshot(
            Request::put("/api/offline-reply/uid-offline")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "item-1001",
                        "enableOfflineReply": true,
                        "offlineReplyText": "你好，请描述你的需求或者直接拍下"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["name"], "不在线回复");
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["results"]["smartTextEdit"]["transport"], "app");
    assert_eq!(payload["results"]["offlineItemEdit"]["transport"], "app");

    let requests = captured.0.lock().unwrap().clone();
    let paths = requests
        .iter()
        .map(|request| request.path.as_str())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"/app/mtop.taobao.idlemessage.profile.smart.edit/1.0/"));
    assert!(paths.contains(&"/app/mtop.taobao.idlemessage.profile.item.offline.config.edit/1.0/"));
    assert!(paths.iter().all(|path| !path.starts_with("/h5/")));

    let offline = requests
        .iter()
        .find(|request| {
            request.path == "/app/mtop.taobao.idlemessage.profile.item.offline.config.edit/1.0/"
        })
        .expect("offline item edit request captured");
    let offline_data = parse_mtop_data(&offline.body);
    let offline_req: Value = serde_json::from_str(offline_data["req"].as_str().unwrap()).unwrap();
    assert_eq!(offline_req["itemId"], "item-1001");
    assert_eq!(offline_req["enableOfflineReply"], true);

    let smart = requests
        .iter()
        .find(|request| request.path == "/app/mtop.taobao.idlemessage.profile.smart.edit/1.0/")
        .expect("smart text edit request captured");
    let smart_data = parse_mtop_data(&smart.body);
    let smart_req: Value = serde_json::from_str(smart_data["req"].as_str().unwrap()).unwrap();
    assert_eq!(smart_req["id"], "offline");
    assert_eq!(smart_req["text"], "你好，请描述你的需求或者直接拍下");
    assert!(smart_req.get("open").is_none());

    let _ = shutdown_tx.send(());
    set_sign_license_context(None);
    clear_env();
}

#[tokio::test]
async fn offline_reply_global_text_sync_does_not_require_item_id() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    let captured = CapturedRequests::default();
    let (base_url, shutdown_tx) = spawn_upstream(captured.clone()).await;
    configure_app_transport_env(&base_url);
    set_sign_license_context(Some(test_sign_license_context(&base_url)));

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts()).await;
    let app = build_app(state);

    let response = app
        .oneshot(
            Request::put("/api/offline-reply/uid-offline")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "enableOfflineReply": true,
                        "offlineReplyText": "亲，我现在不在，稍后回复"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["results"]["smartOpenEdit"]["transport"], "app");
    assert_eq!(payload["results"]["smartTextEdit"]["transport"], "app");
    assert!(payload["results"].get("offlineItemEdit").is_none());

    let requests = captured.0.lock().unwrap().clone();
    let smart_requests = requests
        .iter()
        .filter(|request| request.path == "/app/mtop.taobao.idlemessage.profile.smart.edit/1.0/")
        .collect::<Vec<_>>();
    assert_eq!(smart_requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| !request.path.starts_with("/h5/")));

    let smart_reqs = smart_requests
        .iter()
        .map(|request| {
            let smart_data = parse_mtop_data(&request.body);
            serde_json::from_str::<Value>(smart_data["req"].as_str().unwrap()).unwrap()
        })
        .collect::<Vec<_>>();
    assert!(smart_reqs.iter().any(|req| {
        req["id"] == "offline"
            && req["open"] == true
            && req["useNewStyle"] == true
            && req.get("text").is_none()
    }));
    assert!(smart_reqs.iter().any(|req| {
        req["id"] == "offline"
            && req["text"] == "亲，我现在不在，稍后回复"
            && req.get("open").is_none()
    }));

    let _ = shutdown_tx.send(());
    set_sign_license_context(None);
    clear_env();
}

#[tokio::test]
async fn offline_reply_load_uses_app_mtop_for_offline_lists() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    let captured = CapturedRequests::default();
    let (base_url, shutdown_tx) = spawn_upstream(captured.clone()).await;
    configure_app_transport_env(&base_url);
    set_sign_license_context(Some(test_sign_license_context(&base_url)));

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts()).await;
    let app = build_app(state);

    let response = app
        .oneshot(
            Request::get("/api/offline-reply/uid-offline")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["name"], "不在线回复");
    assert_eq!(payload["transport"], "app");
    assert_eq!(payload["results"]["smartList"]["transport"], "app");
    assert_eq!(payload["results"]["offlineList"]["transport"], "app");

    let requests = captured.0.lock().unwrap().clone();
    let paths = requests
        .iter()
        .map(|request| request.path.as_str())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"/app/mtop.taobao.idlemessage.profile.smart.list/1.0/"));
    assert!(paths.contains(&"/app/mtop.taobao.idlemessage.profile.offline.config.list/1.0/"));
    assert!(paths.iter().all(|path| !path.starts_with("/h5/")));

    let offline = requests
        .iter()
        .find(|request| {
            request.path == "/app/mtop.taobao.idlemessage.profile.offline.config.list/1.0/"
        })
        .expect("offline list request captured");
    let offline_data = parse_mtop_data(&offline.body);
    assert_eq!(offline_data["itemId"], "");

    let _ = shutdown_tx.send(());
    set_sign_license_context(None);
    clear_env();
}

async fn spawn_upstream(captured: CapturedRequests) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/app/:api/:version/", get(app_handler).post(app_handler))
        .route("/sign", post(sign_handler))
        .route(
            "/api/licenses/runtime-challenge",
            post(runtime_challenge_handler),
        )
        .with_state(captured);
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

async fn app_handler(
    State(captured): State<CapturedRequests>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    captured.0.lock().unwrap().push(CapturedRequest {
        path,
        body: String::from_utf8_lossy(&body).to_string(),
    });
    axum::Json(json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {}
    }))
}

async fn sign_handler() -> impl IntoResponse {
    axum::Json(json!({
        "signHeaders": {
            "x-sign": "stub-sign",
            "x-sgext": "stub-sgext",
            "x-mini-wua": "stub-mini-wua",
            "x-umt": "stub-umt"
        },
        "requestHeaders": {
            "x-appkey": "21407387",
            "x-t": "1717171717"
        },
        "updatedInput": {
            "t": "1717171717"
        }
    }))
}

async fn runtime_challenge_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({ "ret": ["FAIL_SYS_NOT_FOUND::skip"] })),
    )
}

async fn seed_account(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid, owner_username, nick, token, sid, cookies, browser_cookie_jar,
            status, created_at, updated_at, extra
        )
        VALUES (?, ?, ?, ?, '', ?, '[]', 'active', 0, 0, ?)
        "#,
    )
    .bind("uid-offline")
    .bind("tester")
    .bind("Offline Seller")
    .bind("seed-token_1700000000000")
    .bind(
        json!({
            "_m_h5_tk": "seed-token_1700000000000",
            "_m_h5_tk_enc": "seed-enc",
            "cookie2": "cookie2-value",
            "unb": "uid-offline"
        })
        .to_string(),
    )
    .bind(
        json!({
            "location": {
                "fishAdCode": "430202",
                "gps": "27.867305,113.164448",
                "latitude": "27.867305",
                "longitude": "113.164448"
            }
        })
        .to_string(),
    )
    .execute(pool)
    .await
    .unwrap();
}

fn parse_mtop_data(body: &str) -> Value {
    let encoded = body.strip_prefix("data=").unwrap_or(body);
    let decoded = urlencoding::decode(encoded).expect("decode mtop body");
    serde_json::from_str(&decoded).expect("parse mtop data")
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    set_env("APP_DATA_DIR", &data_dir.display().to_string());
    set_env("JWT_SECRET", "offline-reply-secret");
    set_env("ADMIN_USERNAME", "admin");
    set_env("API_TOKEN", TEST_BEARER);
    clear_env_key("XIANYU_APP_MTOP_GATEWAY");
    clear_env_key("XIANYU_SIGN_SERVICE_URL");
    clear_env_key("XIANYU_LICENSE_SERVER_URL");
}

fn configure_app_transport_env(base_url: &str) {
    set_env("XIANYU_APP_MTOP_GATEWAY", &format!("{base_url}/app"));
    set_env("XIANYU_SIGN_SERVICE_URL", base_url);
    set_env("XIANYU_LICENSE_SERVER_URL", base_url);
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("API_TOKEN");
    clear_env_key("XIANYU_APP_MTOP_GATEWAY");
    clear_env_key("XIANYU_SIGN_SERVICE_URL");
    clear_env_key("XIANYU_LICENSE_SERVER_URL");
}

fn test_sign_license_context(base_url: &str) -> SignLicenseContext {
    SignLicenseContext {
        server_url: base_url.to_string(),
        auth_token: "test-auth-token".to_string(),
        signed_token: "test-signed-token".to_string(),
        license_id: 1,
        username: "tester".to_string(),
        sku: "admin".to_string(),
        account_limit: 999,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-test-1".to_string(),
    }
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
