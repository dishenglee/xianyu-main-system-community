use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "legacy-token";

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    body: String,
}

#[tokio::test]
async fn item_action_routes_prefer_real_upstream() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_item(&xianyu_pool, "uid-action", "item-1").await;

    let polish_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/polish?uid=uid-action")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(polish_response.status(), StatusCode::OK);
    let polish_payload = json_body(polish_response).await;
    assert_eq!(polish_payload["data"]["polishCount"], 3);

    let reduce_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/reduce_price")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-action",
                        "item_id": "item-1",
                        "amount": "8.80"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reduce_response.status(), StatusCode::OK);
    let reduce_payload = json_body(reduce_response).await;
    assert_eq!(reduce_payload["data"]["markdownId"], "live-markdown-1");

    let exposure_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/super_exposure?uid=uid-action")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(exposure_response.status(), StatusCode::OK);
    let exposure_payload = json_body(exposure_response).await;
    assert_eq!(exposure_payload["data"]["remainingTimes"], 1);

    let delete_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/delete")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-action",
                        "item_id": "item-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);
    let delete_payload = json_body(delete_response).await;
    assert_eq!(delete_payload["data"]["deleted"], true);
    seed_item(&xianyu_pool, "uid-action", "item-1").await;

    let downshelf_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/downshelf")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-action",
                        "item_id": "item-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(downshelf_response.status(), StatusCode::OK);
    let downshelf_payload = json_body(downshelf_response).await;
    assert_eq!(downshelf_payload["data"]["itemId"], "item-1");

    let collect_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/collect")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-action",
                        "item_id": "item-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(collect_response.status(), StatusCode::OK);
    let collect_payload = json_body(collect_response).await;
    assert_eq!(collect_payload["data"]["collected"], true);

    let comment_delete_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/comment/delete")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-action",
                        "item_id": "item-1",
                        "comment_id": "comment-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(comment_delete_response.status(), StatusCode::OK);
    let comment_delete_payload = json_body(comment_delete_response).await;
    assert_eq!(comment_delete_payload["data"]["deletedId"], "comment-1");

    let captured = upstream.requests.lock().unwrap().clone();
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idle.item.polish/2.0/"));
    let markdown = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.taobao.idle.fast.markdown/1.0/")
        .unwrap();
    let markdown_data = parse_mtop_data(&markdown.body);
    assert_eq!(markdown_data["itemId"], "item-1");
    assert_eq!(markdown_data["amount"], "110");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn reduce_price_returns_business_failure_when_result_false() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream_result_false(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_item(&xianyu_pool, "uid-action", "item-1").await;

    let reduce_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/reduce_price")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-action",
                        "item_id": "item-1",
                        "amount": "0.10"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(reduce_response.status(), StatusCode::OK);
    let reduce_payload = json_body(reduce_response).await;
    assert_eq!(
        reduce_payload["ret"][0],
        "FAIL_BIZ_MARKDOWN::商品价格不能低于或等于2人小刀价格哦～"
    );
    assert_eq!(reduce_payload["data"]["result"], false);

    let _ = shutdown_tx.send(());
}

async fn spawn_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler))
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

async fn spawn_upstream_result_false(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler_result_false))
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
    _headers: HeaderMap,
    Query(_query): Query<HashMap<String, String>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        body: String::from_utf8_lossy(&body).to_string(),
    });

    match path.as_str() {
        "/h5/mtop.taobao.idle.item.polish/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "polishCount": 3 }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.fast.markdown/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "markdownId": "live-markdown-1" }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.super.exposure/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "remainingTimes": 1 }
        }))
        .into_response(),
        "/h5/com.taobao.idle.item.delete/1.1/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "deleted": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.item.downshelf/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "itemId": "item-1" }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.collect.item/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "collected": true }
        }))
        .into_response(),
        "/h5/com.taobao.idle.comment.delete/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "deletedId": "comment-1" }
        }))
        .into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({ "ret": ["FAIL_SYS_NOT_FOUND::missing"] })),
        )
            .into_response(),
    }
}

async fn h5_handler_result_false(
    State(state): State<UpstreamState>,
    _headers: HeaderMap,
    Query(_query): Query<HashMap<String, String>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        body: String::from_utf8_lossy(&body).to_string(),
    });

    match path.as_str() {
        "/h5/mtop.taobao.idle.fast.markdown/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "result": false,
                "msg": "商品价格不能低于或等于2人小刀价格哦～"
            }
        }))
        .into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({ "ret": ["FAIL_SYS_NOT_FOUND::missing"] })),
        )
            .into_response(),
    }
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::set_var("API_TOKEN", TEST_BEARER);
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

async fn seed_account(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid,
            nick,
            token,
            sid,
            cookies,
            browser_cookie_jar,
            status,
            created_at,
            updated_at,
            cookie_refreshed_at,
            last_session_check_at,
            last_session_ok_at,
            first_session_fail_at,
            last_session_status,
            last_session_reason,
            extra,
            security_score,
            security_score_updated_at,
            restriction_end,
            coin_balance,
            violation_count,
            fish_power,
            seller_30d_sold
        )
        VALUES (?, ?, ?, '', ?, '[]', 'active', 0, 0, 0, 0, 0, 0, '', '', ?, 95, 0, '', 0, 0, 0, 0)
        "#,
    )
    .bind("uid-action")
    .bind("Action Seller")
    .bind("seed-action-token_1700000000000")
    .bind(
        json!({
            "_m_h5_tk": "seed-action-token_1700000000000",
            "_m_h5_tk_enc": "seed-action-enc",
            "cookie2": "cookie2-value",
            "unb": "uid-action"
        })
        .to_string(),
    )
    .bind(
        json!({
            "location": {
                "fishAdCode": "330106",
                "gps": "30.2741,120.1551",
                "latitude": "30.2741",
                "longitude": "120.1551"
            }
        })
        .to_string(),
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_item(pool: &SqlitePool, uid: &str, item_id: &str) {
    sqlx::query(
        r#"
        INSERT INTO account_items (
            uid, item_id, title, price, status, online, detail_json, created_at, updated_at
        )
        VALUES (?, ?, ?, '9.90', 'ON_SALE', 1, '{}', 0, 0)
        "#,
    )
    .bind(uid)
    .bind(item_id)
    .bind(format!("商品 {item_id}"))
    .execute(pool)
    .await
    .unwrap();
}

fn with_auth(mut request: Request<Body>) -> Request<Body> {
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {TEST_BEARER}")).unwrap(),
    );
    request
}

fn parse_mtop_data(body: &str) -> Value {
    let encoded = body
        .strip_prefix("data=")
        .unwrap_or_else(|| panic!("unexpected mtop body: {body}"));
    let decoded = urlencoding::decode(encoded).expect("decode mtop data");
    serde_json::from_str(&decoded).expect("parse mtop data")
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
