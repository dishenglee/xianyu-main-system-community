use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    extract::{Query, Request, State},
    http::{HeaderMap, StatusCode},
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
    paths: Arc<Mutex<Vec<String>>>,
}

#[tokio::test]
async fn comment_routes_fallback_to_wx_when_h5_fails() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", format!("{base_url}/wx"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_account(&accounts_pool).await;
    seed_item(&xianyu_pool).await;

    let list_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/comment/list")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-wx-comment",
                        "item_id": "12345"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = json_body(list_response).await;
    assert_eq!(list_payload["data"]["comments"][0]["content"], "wx评论");

    let publish_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/comment/publish")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-wx-comment",
                        "item_id": "12345",
                        "content": "新的wx评论"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(publish_response.status(), StatusCode::OK);
    let publish_payload = json_body(publish_response).await;
    assert_eq!(publish_payload["data"]["commentId"], "wx-comment-1");

    let delete_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/comment/delete")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-wx-comment",
                        "item_id": "12345",
                        "comment_id": "555"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);
    let delete_payload = json_body(delete_response).await;
    assert_eq!(delete_payload["data"]["deletedId"], "555");

    let captured_paths = upstream.paths.lock().unwrap().clone();
    assert!(
        captured_paths.contains(&"/wx/h5/mtop.taobao.idle.weixin.comment.list/1.0/".to_string())
    );
    assert!(captured_paths.contains(&"/wx/h5/mtop.taobao.idle.weixin.detail/1.0/".to_string()));
    assert!(
        captured_paths.contains(&"/wx/h5/mtop.taobao.idle.weixin.comment.publish/1.0/".to_string())
    );
    assert!(
        captured_paths.contains(&"/wx/h5/mtop.taobao.idle.weixin.comment.delete/1.0/".to_string())
    );

    let _ = shutdown_tx.send(());
}

async fn spawn_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler))
        .route("/wx/h5/:api/:version/", post(wx_handler))
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
    state.paths.lock().unwrap().push(path.clone());
    let _body = request.into_body().collect().await.unwrap().to_bytes();

    match path.as_str() {
        "/h5/mtop.taobao.idle.comment.list/2.0/" => axum::Json(json!({
            "ret": ["FAIL_SYS_H5_DOWN::h5 unavailable"]
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.comment.publish/2.0/" => axum::Json(json!({
            "ret": ["FAIL_SYS_H5_DOWN::h5 unavailable"]
        }))
        .into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({ "ret": ["FAIL_SYS_NOT_FOUND::missing"] })),
        )
            .into_response(),
    }
}

async fn wx_handler(
    State(state): State<UpstreamState>,
    _headers: HeaderMap,
    Query(_query): Query<HashMap<String, String>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    state.paths.lock().unwrap().push(path.clone());
    let _body = request.into_body().collect().await.unwrap().to_bytes();

    match path.as_str() {
        "/wx/h5/mtop.taobao.idle.weixin.comment.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "comments": [
                    {
                        "commentId": "wx-comment-1",
                        "content": "wx评论"
                    }
                ]
            }
        }))
        .into_response(),
        "/wx/h5/mtop.taobao.idle.weixin.detail/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "title": "WX商品",
                "sellerId": "9988",
                "trackParams": {
                    "sellerId": "9988"
                }
            }
        }))
        .into_response(),
        "/wx/h5/mtop.taobao.idle.weixin.comment.publish/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "commentId": "wx-comment-1"
            }
        }))
        .into_response(),
        "/wx/h5/mtop.taobao.idle.weixin.comment.delete/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "deletedId": "555"
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
            extra,
            status,
            created_at,
            updated_at,
            cookie_refreshed_at
        )
        VALUES (?, ?, ?, ?, ?, ?, 'active', 0, 0, 0)
        "#,
    )
    .bind("uid-wx-comment")
    .bind("WXComment")
    .bind("seed-wx-comment-token_1710000000000")
    .bind("sid-wx-comment")
    .bind(
        json!({
            "_m_h5_tk": "seed-wx-comment-token_1710000000000",
            "_m_h5_tk_enc": "seed-wx-comment-token-enc",
            "cookie2": "cookie-two",
            "t": "token-t"
        })
        .to_string(),
    )
    .bind("{}")
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_item(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO account_items (
            uid, item_id, title, price, status, online, detail_json, created_at, updated_at
        )
        VALUES ('uid-wx-comment', '12345', 'WX商品', '19.90', 'ON_SALE', 1, '{}', 0, 0)
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
