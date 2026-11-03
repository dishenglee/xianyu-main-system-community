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
    bodies: Arc<Mutex<Vec<CapturedRequest>>>,
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    body: String,
    query: HashMap<String, String>,
    cookie: String,
}

#[tokio::test]
async fn item_routes_prefer_real_upstream_for_detail_share_and_comments() {
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
    seed_item(&xianyu_pool, "uid-item", "item-123").await;

    let detail_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/detail")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-item",
                        "item_id": "item-123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(detail_response.status(), StatusCode::OK);
    let detail_payload = json_body(detail_response).await;
    assert_eq!(detail_payload["data"]["itemDO"]["title"], "Live Item");

    let share_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/share_link")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-item",
                        "item_id": "item-123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(share_response.status(), StatusCode::OK);
    let share_payload = json_body(share_response).await;
    assert_eq!(share_payload["url"], "https://share.goofish.com/item-123");
    assert!(share_payload["text"]
        .as_str()
        .unwrap()
        .contains("Live Item"));

    let comment_list_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/comment/list")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-item",
                        "item_id": "item-123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(comment_list_response.status(), StatusCode::OK);
    let comment_list_payload = json_body(comment_list_response).await;
    assert_eq!(
        comment_list_payload["data"]["comments"][0]["content"],
        "真实评论"
    );

    let comment_publish_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/comment/publish")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-item",
                        "item_id": "item-123",
                        "content": "新评论"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(comment_publish_response.status(), StatusCode::OK);
    let comment_publish_payload = json_body(comment_publish_response).await;
    assert_eq!(
        comment_publish_payload["data"]["commentId"],
        "comment-live-1"
    );

    let captured = upstream.bodies.lock().unwrap().clone();
    let publish_request = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.taobao.idle.comment.publish/2.0/")
        .unwrap();
    assert!(publish_request.body.contains("item-123"));
    assert!(publish_request.body.contains("%E6%96%B0%E8%AF%84%E8%AE%BA"));
    assert!(publish_request.cookie.contains("_m_h5_tk=seed-item-token_"));
    assert_eq!(
        publish_request.query.get("api").map(String::as_str),
        Some("mtop.taobao.idle.comment.publish")
    );

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

async fn h5_handler(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    let body_text = String::from_utf8_lossy(&body).to_string();
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    state.bodies.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        body: body_text,
        query,
        cookie,
    });

    match path.as_str() {
        "/h5/mtop.taobao.idle.pc.detail/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "itemDO": {
                    "title": "Live Item",
                    "soldPrice": "88",
                    "shareData": {
                        "shareInfoJsonString": "{\"url\":\"https://share.goofish.com/item-123\"}"
                    }
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.comment.list/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "comments": [
                    {
                        "commentId": "comment-live-1",
                        "content": "真实评论"
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.comment.publish/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "commentId": "comment-live-1",
                "content": "新评论"
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
    env::remove_var("XIANYU_STREAM_UPLOAD_URL");
    env::remove_var("XIANYU_UPLOAD_TAOBAO_URL");
    env::remove_var("XIANYU_WX_GATEWAY");
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
    .bind("uid-item")
    .bind("Item Seller")
    .bind("seed-item-token_1700000000000")
    .bind(
        json!({
            "_m_h5_tk": "seed-item-token_1700000000000",
            "_m_h5_tk_enc": "seed-item-enc",
            "cookie2": "cookie2-value",
            "unb": "uid-item"
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

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
