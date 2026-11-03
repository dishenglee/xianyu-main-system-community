use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app,
    config::AppConfig,
    license_runtime::{get_sign_license_context, set_sign_license_context, SignLicenseContext},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "legacy-token";
const TEST_UPLOAD_BODY_LIMIT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    query: HashMap<String, String>,
    cookie: String,
    body: String,
    headers: HashMap<String, String>,
}

#[tokio::test]
async fn publish_and_upload_routes_prefer_real_upstream_and_persist_cookies() {
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
    env::set_var("XIANYU_UPLOAD_TAOBAO_URL", format!("{base_url}/fileUpload"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_account(&accounts_pool).await;
    seed_item(&xianyu_pool, "uid-upstream", "live-item-1").await;

    let preget_response = app
        .clone()
        .oneshot(
            Request::post("/api/publish/preget?uid=uid-upstream")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preget_response.status(), StatusCode::OK);
    let preget_payload = json_body(preget_response).await;
    assert_eq!(preget_payload["data"]["categories"][0], "真实分类");

    let token_response = app
        .clone()
        .oneshot(
            Request::post("/api/message/token?uid=uid-upstream")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(token_response.status(), StatusCode::OK);
    let token_payload = json_body(token_response).await;
    assert_eq!(token_payload["data"]["token"], "live-im-token");

    let raw_response = app
        .clone()
        .oneshot(
            Request::post("/api/mtop")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "api_name": "mtop.test.echo",
                        "api_version": "1.0",
                        "data": { "hello": "world" }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(raw_response.status(), StatusCode::OK);
    let raw_payload = json_body(raw_response).await;
    assert_eq!(raw_payload["data"]["echo"]["hello"], "world");
    assert_eq!(raw_payload["data"]["attempt"], 2);

    let upload_response = app
        .clone()
        .oneshot(multipart_request(
            "/api/item/upload_image",
            "uid-upstream",
            "cover.jpg",
            "image/jpeg",
            b"fake-jpeg",
        ))
        .await
        .unwrap();
    assert_eq!(upload_response.status(), StatusCode::OK);
    let upload_payload = json_body(upload_response).await;
    assert_eq!(upload_payload["success"], true);
    assert_eq!(
        upload_payload["url"],
        "https://img.alicdn.com/live-item.jpg"
    );

    let large_upload_bytes = vec![0x41_u8; 3 * 1024 * 1024];
    let large_upload_response = app
        .clone()
        .oneshot(multipart_request(
            "/api/item/upload_image",
            "uid-upstream",
            "cover-large.jpg",
            "image/jpeg",
            &large_upload_bytes,
        ))
        .await
        .unwrap();
    let large_upload_status = large_upload_response.status();
    let large_upload_payload = json_body(large_upload_response).await;
    if large_upload_status != StatusCode::OK {
        panic!(
            "large upload returned {large_upload_status}: {}",
            large_upload_payload
        );
    }
    assert_eq!(large_upload_payload["success"], true);
    assert_eq!(
        large_upload_payload["url"],
        "https://img.alicdn.com/live-item.jpg"
    );

    let chat_upload_response = app
        .clone()
        .oneshot(multipart_request(
            "/api/chat/upload_image",
            "uid-upstream",
            "chat.jpg",
            "image/jpeg",
            b"fake-chat-jpeg",
        ))
        .await
        .unwrap();
    assert_eq!(chat_upload_response.status(), StatusCode::OK);
    let chat_upload_payload = json_body(chat_upload_response).await;
    assert_eq!(chat_upload_payload["success"], true);
    assert_eq!(
        chat_upload_payload["url"],
        "https://img.alicdn.com/live-chat.jpg"
    );
    assert_eq!(chat_upload_payload["width"], 320);
    assert_eq!(chat_upload_payload["height"], 240);

    let account_row = sqlx::query("SELECT token, cookies FROM accounts WHERE uid = ?")
        .bind("uid-upstream")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    let persisted_token = account_row.get::<String, _>("token");
    let persisted_cookies = account_row.get::<String, _>("cookies");
    assert!(persisted_token.starts_with("retry-token"));
    assert!(persisted_cookies.contains("\"_m_h5_tk\":\"retry-token_"));

    let captured = upstream.requests.lock().unwrap().clone();
    let raw_requests = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.test.echo/1.0/")
        .collect::<Vec<_>>();
    assert_eq!(raw_requests.len(), 2);
    assert!(raw_requests[0].cookie.contains("_m_h5_tk=seed-token_"));
    assert!(raw_requests[1].cookie.contains("_m_h5_tk=retry-token_"));
    assert_eq!(
        raw_requests[1].query.get("api").map(String::as_str),
        Some("mtop.test.echo")
    );
    assert!(raw_requests[1].query.contains_key("sign"));

    let upload_requests = captured
        .iter()
        .filter(|request| request.path == "/api/upload.api")
        .collect::<Vec<_>>();
    assert_eq!(upload_requests.len(), 3);
    assert!(upload_requests[0].body.contains("bizCode"));
    assert_eq!(
        upload_requests[0].query.get("appkey").map(String::as_str),
        Some("fleamarket")
    );
    assert_eq!(
        upload_requests[1].query.get("appkey").map(String::as_str),
        Some("fleamarket")
    );
    assert_eq!(
        upload_requests[2].query.get("appkey").map(String::as_str),
        Some("xy_chat")
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn trade_routes_send_browser_h5_headers() {
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
    seed_item(&xianyu_pool, "uid-upstream", "live-item-1").await;

    let response = app
        .clone()
        .oneshot(
            Request::get("/api/trade/sold?uid=uid-upstream&page=1&status=all&force=true")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(
        payload["data"]["orderList"].as_array().map(Vec::len),
        Some(1)
    );

    let captured = upstream.requests.lock().unwrap().clone();
    let request = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.taobao.idle.trade.sold.get/6.0/")
        .expect("trade sold request should reach upstream");
    assert_eq!(
        request.headers.get("accept").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        request.headers.get("accept-language").map(String::as_str),
        Some("zh-CN,zh;q=0.9,en;q=0.8")
    );
    assert_eq!(
        request.headers.get("sec-fetch-mode").map(String::as_str),
        Some("cors")
    );
    assert_eq!(
        request.headers.get("sec-fetch-site").map(String::as_str),
        Some("same-site")
    );
    assert_eq!(
        request
            .headers
            .get("sec-ch-ua-platform")
            .map(String::as_str),
        Some("\"macOS\"")
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn captured_h5_item_management_routes_call_verified_upstreams() {
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
    seed_item(&xianyu_pool, "uid-upstream", "live-item-1").await;

    let management_response = app
        .clone()
        .oneshot(
            Request::post("/api/items/management_live")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "page": 1,
                        "page_size": 20,
                        "online_sub_tab": 1
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(management_response.status(), StatusCode::OK);
    let management_payload = json_body(management_response).await;
    assert_eq!(
        management_payload["data"]["api"],
        "mtop.taobao.idle.item.management.query"
    );
    assert_eq!(
        management_payload["data"]["itemList"][0]["itemId"],
        "live-item-1"
    );
    assert_eq!(management_payload["data"]["itemList"][0]["coinRatio"], "20");

    let before_detail = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let detail_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/detail")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "item_id": "live-item-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail_response.status(), StatusCode::OK);
    let detail_payload = json_body(detail_response).await;
    assert_eq!(detail_payload["data"]["itemDO"]["quantity"], "7");

    let quantity_row = sqlx::query(
        "SELECT quantity, quantity_source, quantity_synced_at FROM account_items WHERE uid = ? AND item_id = ?",
    )
    .bind("uid-upstream")
    .bind("live-item-1")
    .fetch_one(&xianyu_pool)
    .await
    .unwrap();
    assert_eq!(quantity_row.get::<Option<i64>, _>("quantity"), Some(7));
    assert_eq!(
        quantity_row.get::<Option<String>, _>("quantity_source"),
        Some("pc.detail:data.itemDO.quantity".to_string())
    );
    let quantity_synced_at = quantity_row
        .get::<Option<i64>, _>("quantity_synced_at")
        .unwrap();
    let after_detail = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!((before_detail..=after_detail).contains(&quantity_synced_at));

    let items_response = app
        .clone()
        .oneshot(
            Request::post("/api/items")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "tab": "ALL",
                        "page": 1,
                        "page_size": 20,
                        "force": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(items_response.status(), StatusCode::OK);
    let items_payload = json_body(items_response).await;
    assert_eq!(items_payload["data"]["itemList"][0]["quantity"], 7);
    assert_eq!(
        items_payload["data"]["itemList"][0]["quantitySource"],
        "pc.detail:data.itemDO.quantity"
    );
    assert_eq!(
        items_payload["data"]["itemList"][0]["quantitySyncedAt"],
        quantity_synced_at
    );

    sqlx::query(
        r#"
        CREATE TRIGGER fail_item_quantity_persist
        BEFORE UPDATE OF quantity ON account_items
        BEGIN
            SELECT RAISE(FAIL, 'forced item quantity persistence failure');
        END
        "#,
    )
    .execute(&xianyu_pool)
    .await
    .unwrap();
    let persistence_failure_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/detail")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "item_id": "live-item-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    sqlx::query("DROP TRIGGER fail_item_quantity_persist")
        .execute(&xianyu_pool)
        .await
        .unwrap();
    assert_eq!(
        persistence_failure_response.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let persistence_failure_payload = json_body(persistence_failure_response).await;
    assert_eq!(
        persistence_failure_payload["detail"],
        "商品详情已获取，但库存同步失败，请稍后重试"
    );
    let persistence_failure_text = persistence_failure_payload.to_string();
    assert!(!persistence_failure_text.contains("forced item quantity persistence failure"));
    assert!(!persistence_failure_text.contains("UPDATE account_items"));

    let reduce_feed_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/reduce_price_feeds")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "item_id": "live-item-1",
                        "suggest_price": "88"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let reduce_feed_status = reduce_feed_response.status();
    if reduce_feed_status != StatusCode::OK {
        let reduce_feed_payload = json_body(reduce_feed_response).await;
        panic!(
            "reduce_price_feeds returned {reduce_feed_status}: {}",
            reduce_feed_payload
        );
    }
    let reduce_feed_payload = json_body(reduce_feed_response).await;
    assert_eq!(
        reduce_feed_payload["data"]["api"],
        "mtop.taobao.idle.reduce.price.feeds.query"
    );
    assert_eq!(
        reduce_feed_payload["data"]["feeds"][0]["title"],
        "建议降价 5 元"
    );

    let recommend_response = app
        .clone()
        .oneshot(
            Request::get("/api/item/seller_recommend?uid=uid-upstream&page=1&page_size=20")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(recommend_response.status(), StatusCode::OK);
    let recommend_payload = json_body(recommend_response).await;
    assert_eq!(
        recommend_payload["data"]["api"],
        "mtop.taobao.idle.activity.sell.seller.recommend"
    );
    assert_eq!(
        recommend_payload["data"]["recOrders"][0]["title"],
        "可转卖订单"
    );

    let awesome_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/awesome_detail")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "uid": "uid-upstream", "item_id": "live-item-1" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(awesome_response.status(), StatusCode::OK);
    let awesome_payload = json_body(awesome_response).await;
    assert_eq!(
        awesome_payload["data"]["api"],
        "mtop.taobao.idle.awesome.detail"
    );
    assert_eq!(awesome_payload["data"]["itemId"], "live-item-1");

    let capabilities_response = app
        .clone()
        .oneshot(
            Request::get("/api/publish/capabilities?uid=uid-upstream")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(capabilities_response.status(), StatusCode::OK);
    let capabilities_payload = json_body(capabilities_response).await;
    assert!(
        capabilities_payload["data"]["successCount"]
            .as_u64()
            .unwrap_or_default()
            >= 5
    );
    assert_eq!(
        capabilities_payload["data"]["results"]["serviceCards"]["api"],
        "mtop.idle.item.publish.service.cards.list"
    );

    let captured = upstream.requests.lock().unwrap().clone();
    for path in [
        "/h5/mtop.taobao.idle.item.management.query/1.0/",
        "/h5/mtop.taobao.idle.pc.detail/1.0/",
        "/h5/mtop.taobao.idle.reduce.price.feeds.query/1.0/",
        "/h5/mtop.taobao.idle.activity.sell.seller.recommend/2.0/",
        "/h5/mtop.taobao.idle.awesome.detail/1.0/",
        "/h5/mtop.idle.idleitem.publish.preget/3.0/",
        "/h5/mtop.idle.idleitem.publish.photo.template/1.0/",
        "/h5/mtop.idle.item.publish.service.cards.list/1.0/",
        "/h5/mtop.taobao.idle.group.item.publish.select.list/1.0/",
        "/h5/mtop.taobao.idle.item.publish.postget/1.0/",
    ] {
        assert!(
            captured.iter().any(|request| request.path == path),
            "missing captured H5 path {path}"
        );
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn item_detail_falls_back_to_cached_row_when_pc_detail_upstream_fails() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    env::set_var("XIANYU_H5_GATEWAY", "http://127.0.0.1:9/h5");
    env::set_var("XIANYU_WX_GATEWAY", "http://127.0.0.1:9/wx");

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_account(&accounts_pool).await;
    seed_item(&xianyu_pool, "uid-upstream", "live-item-1").await;
    sqlx::query(
        "UPDATE account_items SET quantity = 5, quantity_source = 'cached.snapshot', quantity_synced_at = 123 WHERE uid = ? AND item_id = ?",
    )
    .bind("uid-upstream")
    .bind("live-item-1")
    .execute(&xianyu_pool)
    .await
    .unwrap();

    let detail_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/detail")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "item_id": "live-item-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail_response.status(), StatusCode::OK);
    let detail_payload = json_body(detail_response).await;
    assert_eq!(detail_payload["data"]["itemId"], "live-item-1");
    assert_eq!(detail_payload["data"]["quantity"], 5);
    assert_eq!(detail_payload["data"]["quantitySource"], "cached.snapshot");
    assert_eq!(detail_payload["data"]["quantitySyncedAt"], 123);
    assert!(!detail_payload.to_string().contains("http://127.0.0.1:9"));
}

#[tokio::test]
async fn ai_description_route_prefers_h5_generation_transport() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    configure_app_transport_env(&base_url);
    let original_license = get_sign_license_context();
    set_sign_license_context(Some(test_sign_license_context(&base_url)));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/ai_description")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "title": "测试 AI 描述",
                        "desc": "验证 AI 描述主链路",
                        "image_urls": ["https://img.alicdn.com/live-item-1.jpg"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["title"], "测试 AI 描述");
    assert_eq!(payload["data"]["desc"], "来自 H5 的 AI 描述");
    assert_eq!(payload["data"]["source"], "upstream_h5");

    let captured = captured.lock().unwrap().clone();
    assert!(captured
        .iter()
        .any(|request| { request.path == "/h5/mtop.taobao.idle.adv.item.desc.generation/2.0/" }));
    assert!(!captured
        .iter()
        .any(|request| { request.path == "/app/mtop.taobao.idle.adv.item.desc.generation/2.0/" }));

    set_sign_license_context(original_license);
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn ai_description_route_errors_when_h5_generation_unavailable() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let original_h5_gateway = env::var("XIANYU_H5_GATEWAY").ok();
    env::set_var("XIANYU_H5_GATEWAY", "http://127.0.0.1:9/h5");
    let original_license = get_sign_license_context();
    set_sign_license_context(None);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/ai_description")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "title": "罗技 G PRO X 机械键盘",
                        "desc": ""
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let payload = json_body(response).await;
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["detail"], "官方 AI 描述服务暂时不可用，请稍后重试");

    set_sign_license_context(original_license);
    if let Some(value) = original_h5_gateway {
        env::set_var("XIANYU_H5_GATEWAY", value);
    } else {
        env::remove_var("XIANYU_H5_GATEWAY");
    }
}

#[tokio::test]
async fn ai_description_route_surfaces_official_h5_generation_error_text() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    configure_app_transport_env(&base_url);
    let original_license = get_sign_license_context();
    set_sign_license_context(Some(test_sign_license_context(&base_url)));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/ai_description")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "title": "trigger_generation_error_token",
                        "desc": "验证上游原文透传"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let payload = json_body(response).await;
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["detail"], "官方生成错误");

    set_sign_license_context(original_license);
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn ai_description_route_surfaces_official_kgraph_error_text() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    configure_app_transport_env(&base_url);
    let original_license = get_sign_license_context();
    set_sign_license_context(Some(test_sign_license_context(&base_url)));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/ai_description")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "title": "trigger_kgraph_error_token",
                        "desc": "验证分类识别原文透传"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let payload = json_body(response).await;
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["detail"], "官方分类错误");

    set_sign_license_context(original_license);
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn coin_deduction_open_warms_up_via_h5_only() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    configure_app_transport_env(&base_url);
    let original_license = get_sign_license_context();
    set_sign_license_context(Some(test_sign_license_context(&base_url)));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_account(&accounts_pool).await;
    seed_item(&xianyu_pool, "uid-upstream", "live-item-1").await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/coin/deduction/open")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "item_ids": ["live-item-1"],
                        "ratio": "20"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["result"], true);

    let captured = captured.lock().unwrap().clone();
    for path in [
        "/h5/mtop.taobao.idle.coin.deduction.agree.query/1.0/",
        "/h5/mtop.taobao.idle.coin.deduction.penalty.query/1.0/",
        "/h5/mtop.taobao.idle.coin.deduction.list/1.0/",
        "/h5/mtop.taobao.idle.coin.deduction.new.open/1.0/",
    ] {
        assert!(
            captured.iter().any(|request| request.path == path),
            "missing captured H5 path {path}"
        );
    }
    for path in [
        "/app/mtop.taobao.idle.coin.deduction.agree.query/1.0/",
        "/app/mtop.taobao.idle.coin.deduction.penalty.query/1.0/",
        "/app/mtop.taobao.idle.coin.deduction.list/1.0/",
    ] {
        assert!(
            !captured.iter().any(|request| request.path == path),
            "unexpected app warm-up path {path}"
        );
    }

    set_sign_license_context(original_license);
    let _ = shutdown_tx.send(());
}

async fn spawn_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler))
        .route("/app/:api/:version/", post(app_handler))
        .route("/sign", post(sign_handler))
        .route(
            "/api/me/xianyu-account-bindings/resolve",
            post(resolve_bindings_handler),
        )
        .route(
            "/api/licenses/runtime-challenge",
            post(runtime_challenge_handler),
        )
        .route("/api/upload.api", post(upload_handler))
        .layer(DefaultBodyLimit::max(TEST_UPLOAD_BODY_LIMIT_BYTES))
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
    request: Request<axum::body::Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8_lossy(&body).to_string();
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        query,
        cookie: cookie.clone(),
        body: body_str.clone(),
        headers: captured_headers(&headers),
    });

    match path.as_str() {
        "/h5/mtop.idle.idleitem.publish.preget/3.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "categories": ["真实分类"], "draftEnabled": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.item.management.query/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "itemCards": [{
                    "id": "live-item-1",
                    "title": "实时商品",
                    "price": "99.00",
                    "priceTags": {
                        "priceSuffixLabelInfoDO": {
                            "text": "闲鱼币抵扣20%"
                        }
                    },
                    "picUrl": "https://img.alicdn.com/live-item-1.jpg",
                    "online": true,
                    "browseCount": 12,
                    "collectNum": 3,
                    "actions": [{ "actionType": 3, "actionName": "降价", "enable": "1" }]
                }],
                "nextPage": false,
                "totalCount": 1
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.pc.detail/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "quantity": 99,
                "itemDO": {
                    "itemId": "live-item-1",
                    "title": "实时详情",
                    "quantity": "7"
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.reduce.price.feeds.query/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "feeds": [{ "title": "建议降价 5 元", "suggestPrice": "88" }],
                "itemId": "live-item-1"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.activity.sell.seller.recommend/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "recOrders": [{ "title": "可转卖订单", "tradeId": "trade-1" }]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.awesome.detail/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "itemId": "live-item-1", "title": "实时详情" }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.awesome.detail.unit/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "itemId": "live-item-1", "modules": [] }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.kgraph.property.recommend/2.0/" => {
            if body_str.contains("trigger_kgraph_error_token") {
                axum::Json(json!({
                    "ret": ["FAIL_BIZ_KGRAPH_REJECT::官方分类错误"]
                }))
                .into_response()
            } else {
                axum::Json(json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": {
                        "cardList": [{
                            "cardData": {
                                "propertyId": "-10000",
                                "propertyName": "分类",
                                "valuesList": [{
                                    "catId": "50015123",
                                    "catName": "文章/软文写作服务",
                                    "channelCatId": "201460608",
                                    "channelCatName": "文章/软文写作服务",
                                    "tbCatId": "50015123"
                                }]
                            }
                        }]
                    }
                }))
                .into_response()
            }
        }
        "/h5/mtop.taobao.idle.adv.item.desc.generation/2.0/" => {
            if body_str.contains("trigger_generation_error_token") {
                axum::Json(json!({
                    "ret": ["FAIL_BIZ_AI_DESC_LIMIT::官方生成错误"]
                }))
                .into_response()
            } else {
                axum::Json(json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": {
                        "description": "来自 H5 的 AI 描述",
                        "requestId": "h5-ai-desc-1"
                    }
                }))
                .into_response()
            }
        }
        "/h5/mtop.idle.idleitem.publish.photo.template/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "templates": [{ "name": "主图模板" }] }
        }))
        .into_response(),
        "/h5/mtop.idle.item.publish.service.cards.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "serviceCardSort": ["idleCoinDeduction"] }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.group.item.publish.select.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "groups": [{ "id": "0", "name": "默认分组" }] }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.item.publish.postGet/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "postGet": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.item.publish.postget/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "postget": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idleitem.prepublish.check/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "canPublish": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idleitem.badwords.prepubcheck/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "safe": true, "matched": [] }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.pc.login.token/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "token": "live-im-token", "expiresIn": 7200 }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.deduction.agree.query/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "agreeFlag": 1 }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.deduction.penalty.query/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "withPenalty": false }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.deduction.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "cardList": [{
                    "id": "live-item-1",
                    "coinDeductionInfo": {
                        "open": false,
                        "ratio": "20",
                        "canOpen": true
                    }
                }]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.deduction.new.open/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "result": true }
        }))
        .into_response(),
        "/h5/mtop.test.echo/1.0/" => {
            if cookie.contains("retry-token_") {
                (
                    [("set-cookie", "_m_h5_tk=retry-token_1710000000000; Path=/")],
                    axum::Json(json!({
                        "ret": ["SUCCESS::调用成功"],
                        "data": {
                            "echo": { "hello": "world" },
                            "attempt": 2
                        }
                    })),
                )
                    .into_response()
            } else {
                (
                    [("set-cookie", "_m_h5_tk=retry-token_1710000000000; Path=/")],
                    axum::Json(json!({
                        "ret": ["FAIL_SYS_TOKEN_EXPIRED::令牌过期"]
                    })),
                )
                    .into_response()
            }
        }
        "/h5/mtop.taobao.idle.trade.sold.get/6.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "items": [{
                    "commonData": {
                        "orderId": 4502276319054045503_i64,
                        "orderIdStr": "4502276319054045503",
                        "peerUserId": 17295474531_i64,
                        "tradeStatusEnum": "seller_to_send"
                    },
                    "content": {
                        "data": {
                            "detailInfo": {
                                "auctionTitle": "罗技G PRO 60无线机械键盘",
                                "auctionPic": "https://img.alicdn.com/item.jpg"
                            },
                            "priceInfo": {
                                "price": "0.01"
                            }
                        }
                    },
                    "head": {
                        "data": {
                            "createTime": "2026-04-11 13:44:06",
                            "statusViewMsg": "等待卖家发货",
                            "userInfo": {
                                "userName": "糯叽叽的墨染呀",
                                "avatarUrl": "https://img.alicdn.com/avatar.jpg"
                            }
                        }
                    }
                }],
                "totalCount": 1
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

async fn app_handler(
    State(state): State<UpstreamState>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        query: HashMap::new(),
        cookie: String::new(),
        body: String::from_utf8_lossy(&body).to_string(),
        headers: HashMap::new(),
    });

    match path.as_str() {
        "/app/mtop.taobao.idle.adv.item.desc.generation/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "description": "来自 APP 的 AI 描述",
                "requestId": "app-ai-desc-1"
            }
        }))
        .into_response(),
        "/app/mtop.taobao.idle.coin.deduction.agree.query/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "agreeFlag": 1 }
        }))
        .into_response(),
        "/app/mtop.taobao.idle.coin.deduction.penalty.query/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "withPenalty": false }
        }))
        .into_response(),
        "/app/mtop.taobao.idle.coin.deduction.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "cardList": [] }
        }))
        .into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({ "ret": ["FAIL_SYS_NOT_FOUND::missing"] })),
        )
            .into_response(),
    }
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
            "x-appkey": "12574478",
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

async fn resolve_bindings_handler(body: Bytes) -> impl IntoResponse {
    let payload: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
    let results = payload
        .get("account_uids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|account_uid| {
            json!({
                "account_uid": account_uid,
                "status": "owned_by_me",
                "owner_username": "tester",
                "message": ""
            })
        })
        .collect::<Vec<_>>();
    axum::Json(Value::Array(results))
}

async fn upload_handler(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> impl IntoResponse {
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: "/api/upload.api".to_string(),
        query: query.clone(),
        cookie,
        body: String::from_utf8_lossy(&body).to_string(),
        headers: captured_headers(&headers),
    });

    let appkey = query.get("appkey").cloned().unwrap_or_default();
    let (url, pix) = if appkey == "xy_chat" {
        ("https://img.alicdn.com/live-chat.jpg", "320x240")
    } else {
        ("https://img.alicdn.com/live-item.jpg", "")
    };

    axum::Json(json!({
        "object": {
            "url": url,
            "pix": pix,
            "success": true
        }
    }))
}

fn captured_headers(headers: &HeaderMap) -> HashMap<String, String> {
    [
        "accept",
        "accept-language",
        "sec-ch-ua",
        "sec-ch-ua-mobile",
        "sec-ch-ua-platform",
        "sec-fetch-dest",
        "sec-fetch-mode",
        "sec-fetch-site",
        "origin",
        "referer",
    ]
    .into_iter()
    .filter_map(|name| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(|value| (name.to_string(), value.to_string()))
    })
    .collect()
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::set_var("API_TOKEN", TEST_BEARER);
    env::remove_var("XIANYU_APP_MTOP_GATEWAY");
    env::remove_var("XIANYU_SIGN_SERVICE_URL");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
}

fn configure_app_transport_env(base_url: &str) {
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_APP_MTOP_GATEWAY", format!("{base_url}/app"));
    env::set_var("XIANYU_SIGN_SERVICE_URL", base_url);
    env::set_var("XIANYU_LICENSE_SERVER_URL", base_url);
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

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

async fn seed_account(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid,
            owner_username,
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
        VALUES (?, ?, ?, ?, '', ?, '[]', 'active', 0, 0, 0, 0, 0, 0, '', '', ?, 95, 0, '', 0, 0, 0, 0)
        "#,
    )
    .bind("uid-upstream")
    .bind("tester")
    .bind("Seller Upstream")
    .bind("seed-token_1700000000000")
    .bind(
        json!({
            "_m_h5_tk": "seed-token_1700000000000",
            "_m_h5_tk_enc": "seed-enc",
            "cookie2": "cookie2-value",
            "unb": "uid-upstream"
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
    .bind(format!("Upstream Item {item_id}"))
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn with_auth(mut request: Request<Body>) -> Request<Body> {
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {TEST_BEARER}")).unwrap(),
    );
    request
}

fn multipart_request(
    uri: &str,
    uid: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
) -> Request<axum::body::Body> {
    let boundary = "----xianyu-upstream-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"uid\"\r\n\r\n");
    body.extend_from_slice(uid.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    Request::post(uri)
        .header("authorization", format!("Bearer {TEST_BEARER}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(axum::body::Body::from(body))
        .unwrap()
}
