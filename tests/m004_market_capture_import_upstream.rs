use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::{Body, Bytes},
    extract::{Query, Request, State},
    http::{header::CONTENT_TYPE, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "legacy-token";

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    upload_count: Arc<Mutex<usize>>,
    fail_import_detail: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    method: String,
    path: String,
}

#[tokio::test]
async fn capture_route_persists_snapshot_scores_and_upserts_duplicate_comments() {
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
    seed_item(&xianyu_pool, "uid-capture", "778899").await;

    for _ in 0..2 {
        let capture_response = app
            .clone()
            .oneshot(
                Request::post("/api/item/comment/capture")
                    .header("authorization", format!("Bearer {TEST_BEARER}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "uid": "uid-capture",
                            "item_id": "778899"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(capture_response.status(), StatusCode::OK);
        let capture_payload = json_body(capture_response).await;
        assert_eq!(capture_payload["source_type"], "h5");
        assert_eq!(
            capture_payload["source_api"],
            "mtop.taobao.idle.comment.list"
        );
        assert_eq!(capture_payload["quality_counts"]["good"], 1);
        assert_eq!(capture_payload["quality_counts"]["review"], 1);
        assert_eq!(capture_payload["quality_counts"]["bad"], 1);
        assert_eq!(capture_payload["quality_counts"]["total"], 3);
        assert_eq!(capture_payload["items"].as_array().map(Vec::len), Some(3));
    }

    let rows = sqlx::query(
        r#"
        SELECT content, quality_label, category, source_type, source_api
        FROM item_comment_snapshots
        WHERE uid = ? AND item_id = ?
        ORDER BY content ASC
        "#,
    )
    .bind("uid-capture")
    .bind("778899")
    .fetch_all(&xianyu_pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 3);

    let mut quality_by_content = HashMap::new();
    let mut category_by_content = HashMap::new();
    for row in rows {
        let content = row.get::<String, _>("content");
        quality_by_content.insert(content.clone(), row.get::<String, _>("quality_label"));
        category_by_content.insert(content.clone(), row.get::<String, _>("category"));
        assert_eq!(row.get::<String, _>("source_type"), "h5");
        assert_eq!(
            row.get::<String, _>("source_api"),
            "mtop.taobao.idle.comment.list"
        );
    }

    assert_eq!(
        quality_by_content
            .get("物流很快，包装也不错")
            .map(String::as_str),
        Some("good")
    );
    assert_eq!(
        quality_by_content.get("价格还可以吧").map(String::as_str),
        Some("review")
    );
    assert_eq!(
        quality_by_content.get("vx???").map(String::as_str),
        Some("bad")
    );
    assert_eq!(
        category_by_content
            .get("物流很快，包装也不错")
            .map(String::as_str),
        Some("物流")
    );
    assert_eq!(
        category_by_content.get("价格还可以吧").map(String::as_str),
        Some("价格")
    );

    let item_comment_count = sqlx::query(
        "SELECT COUNT(*) AS count FROM item_comment_snapshots WHERE uid = ? AND item_id = ?",
    )
    .bind("uid-capture")
    .bind("778899")
    .fetch_one(&xianyu_pool)
    .await
    .unwrap()
    .get::<i64, _>("count");
    assert_eq!(item_comment_count, 3);

    let captured = upstream.requests.lock().unwrap().clone();
    let comment_calls = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.taobao.idle.comment.list/2.0/")
        .count();
    assert_eq!(comment_calls, 2);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn import_link_route_resolves_short_link_and_reuploads_images() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", format!("{base_url}/wx"));
    env::set_var(
        "XIANYU_STREAM_UPLOAD_URL",
        format!("{base_url}/api/upload.api"),
    );

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let import_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/import_link")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-capture",
                        "text": format!("口令短链 {base_url}/short/demo")
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(import_response.status(), StatusCode::OK);
    let import_payload = json_body(import_response).await;
    assert_eq!(import_payload["itemId"], "778899");
    assert_eq!(import_payload["title"], "Imported Chair");
    assert_eq!(import_payload["desc"], "Imported desc");
    assert_eq!(import_payload["price"], "99");
    assert_eq!(import_payload["origPrice"], "129");
    assert_eq!(
        import_payload["imageUrls"],
        json!([
            "https://img.alicdn.com/reuploaded-1.jpg",
            "https://img.alicdn.com/reuploaded-2.png"
        ])
    );

    let captured = upstream.requests.lock().unwrap().clone();
    assert!(captured
        .iter()
        .any(|request| request.method == "GET" && request.path == "/short/demo"));
    assert!(captured
        .iter()
        .any(|request| request.method == "POST"
            && request.path == "/h5/mtop.taobao.idle.pc.detail/1.0/"));
    assert!(captured
        .iter()
        .any(|request| request.method == "GET" && request.path == "/images/source-1.jpg"));
    assert!(captured
        .iter()
        .any(|request| request.method == "GET" && request.path == "/images/source-2.png"));
    assert_eq!(
        captured
            .iter()
            .filter(|request| request.method == "POST" && request.path == "/api/upload.api")
            .count(),
        2
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn import_link_route_accepts_branded_short_links_in_share_text() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", format!("{base_url}/wx"));
    env::set_var(
        "XIANYU_STREAM_UPLOAD_URL",
        format!("{base_url}/api/upload.api"),
    );

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let import_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/import_link")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-capture",
                        "text": format!(
                            "【闲鱼】{base_url}/short/demo MF287 「快来捡漏【Apple Mac mini 2023款，M2芯片，8G内存】」 点击链接直接打开"
                        )
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(import_response.status(), StatusCode::OK);
    let import_payload = json_body(import_response).await;
    assert_eq!(import_payload["itemId"], "778899");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn import_link_route_accepts_goofish_item_links_with_extra_query_params() {
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
    seed_account(&accounts_pool).await;

    let import_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/import_link")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-capture",
                        "text": "https://www.goofish.com/item?spm=a21ybx.item.itemCnxh.2.28193da6dj8cJT&id=1058988547353&categoryId=0"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(import_response.status(), StatusCode::OK);
    let import_payload = json_body(import_response).await;
    assert_eq!(import_payload["itemId"], "1058988547353");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn import_link_route_rejects_failed_detail_import_without_persisting_fake_item() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    *upstream.fail_import_detail.lock().unwrap() = true;
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", format!("{base_url}/wx"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_account(&accounts_pool).await;

    let import_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/import_link")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-capture",
                        "text": "https://www.goofish.com/item?id=778899"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(import_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let import_payload = json_body(import_response).await;
    assert_eq!(import_payload["detail"], "导入商品详情失败，请稍后重试");

    let fake_count = sqlx::query("SELECT COUNT(*) AS count FROM account_items WHERE item_id = ?")
        .bind("778899")
        .fetch_one(&xianyu_pool)
        .await
        .unwrap()
        .get::<i64, _>("count");
    assert_eq!(fake_count, 0);

    let _ = shutdown_tx.send(());
}

async fn spawn_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler))
        .route("/wx/h5/:api/:version/", post(wx_handler))
        .route("/api/upload.api", post(upload_handler))
        .route("/short/demo", get(short_link_handler))
        .route("/images/source-1.jpg", get(image_one_handler))
        .route("/images/source-2.png", get(image_two_handler))
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
    let base_url = request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok())
        .map(|host| format!("http://{host}"))
        .unwrap_or_default();
    state.requests.lock().unwrap().push(CapturedRequest {
        method: "POST".to_string(),
        path: path.clone(),
    });
    let _body = request.into_body().collect().await.unwrap().to_bytes();

    match path.as_str() {
        "/h5/mtop.taobao.idle.comment.list/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "data": {
                    "rows": [
                        {
                            "commentDO": {
                                "commentId": "comment-cap-1",
                                "publishTime": "1710000000",
                                "content": "物流很快，包装也不错"
                            },
                            "author": {
                                "id": "author-1",
                                "nick": "评论用户A"
                            }
                        },
                        {
                            "commentDO": {
                                "commentId": "comment-cap-2",
                                "publishTime": "1710000001",
                                "content": "价格还可以吧"
                            },
                            "author": {
                                "id": "author-2",
                                "nick": "评论用户B"
                            }
                        },
                        {
                            "commentDO": {
                                "commentId": "comment-cap-3",
                                "publishTime": "1710000002",
                                "content": "vx???"
                            },
                            "author": {
                                "id": "author-3",
                                "nick": "评论用户C"
                            }
                        }
                    ]
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.pc.detail/1.0/" => {
            if *state.fail_import_detail.lock().unwrap() {
                axum::Json(json!({
                    "ret": ["FAIL_BIZ_DETAIL_UNAVAILABLE::detail failed"],
                }))
                .into_response()
            } else {
                axum::Json(json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": {
                        "itemDO": {
                            "itemId": "778899",
                            "title": "Imported Chair",
                            "desc": "Imported desc",
                            "soldPrice": "99.00",
                            "originalPrice": "129.00",
                            "imageInfos": [
                                { "url": format!("{base_url}/images/source-1.jpg") },
                                { "url": format!("{base_url}/images/source-2.png") }
                            ]
                        }
                    }
                }))
                .into_response()
            }
        }
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
    state.requests.lock().unwrap().push(CapturedRequest {
        method: "POST".to_string(),
        path,
    });
    let _body = request.into_body().collect().await.unwrap().to_bytes();
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({ "ret": ["FAIL_SYS_NOT_FOUND::missing"] })),
    )
        .into_response()
}

async fn short_link_handler(
    State(state): State<UpstreamState>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let base_url = request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok())
        .map(|host| format!("http://{host}"))
        .unwrap_or_default();
    state.requests.lock().unwrap().push(CapturedRequest {
        method: "GET".to_string(),
        path,
    });
    (
        [(CONTENT_TYPE, "text/html; charset=utf-8")],
        format!(
            "<html><script>var url = 'https://www.goofish.com/item?id=778899';</script><body>{base_url}</body></html>"
        ),
    )
}

async fn image_one_handler(
    State(state): State<UpstreamState>,
    request: Request<Body>,
) -> impl IntoResponse {
    state.requests.lock().unwrap().push(CapturedRequest {
        method: "GET".to_string(),
        path: request.uri().path().to_string(),
    });
    (
        [(CONTENT_TYPE, "image/jpeg")],
        Bytes::from_static(b"image-one"),
    )
}

async fn image_two_handler(
    State(state): State<UpstreamState>,
    request: Request<Body>,
) -> impl IntoResponse {
    state.requests.lock().unwrap().push(CapturedRequest {
        method: "GET".to_string(),
        path: request.uri().path().to_string(),
    });
    (
        [(CONTENT_TYPE, "image/png")],
        Bytes::from_static(b"image-two"),
    )
}

async fn upload_handler(
    State(state): State<UpstreamState>,
    _headers: HeaderMap,
    Query(_query): Query<HashMap<String, String>>,
    body: Bytes,
) -> impl IntoResponse {
    state.requests.lock().unwrap().push(CapturedRequest {
        method: "POST".to_string(),
        path: "/api/upload.api".to_string(),
    });
    assert!(!body.is_empty());

    let mut counter = state.upload_count.lock().unwrap();
    *counter += 1;
    let upload_url = match *counter {
        1 => "https://img.alicdn.com/reuploaded-1.jpg",
        _ => "https://img.alicdn.com/reuploaded-2.png",
    };

    axum::Json(json!({
        "object": {
            "url": upload_url,
            "success": true
        }
    }))
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
    .bind("uid-capture")
    .bind("Capture")
    .bind("seed-capture-token_1710000000000")
    .bind("sid-capture")
    .bind(
        json!({
            "_m_h5_tk": "seed-capture-token_1710000000000",
            "_m_h5_tk_enc": "seed-capture-token-enc",
            "cookie2": "cookie-two",
            "t": "token-t"
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
        VALUES (?, ?, ?, '99.00', 'ON_SALE', 1, '{}', 0, 0)
        "#,
    )
    .bind(uid)
    .bind(item_id)
    .bind(format!("Capture Item {item_id}"))
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
