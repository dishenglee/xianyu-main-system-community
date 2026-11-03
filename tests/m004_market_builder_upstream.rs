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
    data: Value,
    query: HashMap<String, String>,
}

#[tokio::test]
async fn builder_style_item_routes_prefer_real_upstream_and_build_expected_payloads() {
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
    seed_cached_item(&xianyu_pool, "uid-builder", "item-123").await;

    let kgraph_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/kgraph")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-builder",
                        "title": "Live Chair",
                        "desc": "Solid wood chair"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(kgraph_response.status(), StatusCode::OK);
    let kgraph_payload = json_body(kgraph_response).await;
    assert_eq!(
        kgraph_payload["data"]["cardList"][0]["propertyId"],
        "-10000"
    );
    assert_eq!(
        kgraph_payload["data"]["cardList"][0]["propertyName"],
        "分类"
    );

    let publish_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/publish")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-builder",
                        "title": "Live Chair",
                        "short_title": "Chair",
                        "desc": "Solid wood chair",
                        "price": "128.00",
                        "orig_price": "199.00",
                        "image_urls": ["https://img.alicdn.com/source-chair.jpg"],
                        "simple_item": false,
                        "no_bargain": true,
                        "can_free_shipping": false,
                        "division_id": "330106",
                        "city": "杭州市",
                        "province": "浙江省",
                        "area": "西湖区",
                        "latitude": "30.2741",
                        "longitude": "120.1551"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(publish_response.status(), StatusCode::OK);
    let publish_payload = json_body(publish_response).await;
    assert_eq!(publish_payload["data"]["itemId"], "new-live-item");

    let edit_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/edit")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-builder",
                        "item_id": "item-123",
                        "title": "Edited Live Chair",
                        "short_title": "Edited Chair",
                        "desc": "Edited desc",
                        "price": "99.00",
                        "orig_price": "149.00",
                        "no_bargain": true,
                        "can_free_shipping": true,
                        "image_urls": ["https://img.alicdn.com/edited-chair.jpg"],
                        "detail": {
                            "itemId": "item-123",
                            "itemTextDTO": { "title": "Old Chair", "shortTitle": "Old", "desc": "Old desc" },
                            "itemPriceDTO": { "priceInCent": "12800", "origPriceInCent": "19900", "noBargain": false },
                            "itemAddrDTO": {
                                "divisionId": "330106",
                                "city": "杭州市",
                                "prov": "浙江省",
                                "area": "西湖区",
                                "gps": "30.2741,120.1551",
                                "addressType": 5
                            },
                            "itemCatDTO": {
                                "catId": "50012010",
                                "tbCatId": "50012010",
                                "channelCatId": "12345",
                                "catName": "椅子",
                                "leafId": "60001"
                            },
                            "itemLabelExtList": [],
                            "itemPostFeeDTO": { "canFreeShipping": false, "supportFreight": true },
                            "itemTypeStr": "b",
                            "simpleItem": "false",
                            "quantity": "1"
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(edit_response.status(), StatusCode::OK);
    let edit_payload = json_body(edit_response).await;
    assert_eq!(edit_payload["data"]["updated"], true);
    assert_eq!(edit_payload["data"]["itemId"], "item-123");

    let open_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/open")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-builder",
                        "item_id": "item-123",
                        "item_context": {
                            "itemId": "item-123",
                            "divisionId": "330106",
                            "city": "杭州市",
                            "province": "浙江省",
                            "area": "西湖区",
                            "gps": "30.2741,120.1551",
                            "tbCatId": "50012010",
                            "channelCatId": "12345",
                            "leafId": "60001",
                            "addressType": 5,
                            "editorVersion": "0.0.1",
                            "attributesMap": {
                                "noBargain": "true",
                                "supportFreight": "true",
                                "freeShipping": "false"
                            }
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(open_response.status(), StatusCode::OK);
    let open_payload = json_body(open_response).await;
    assert_eq!(open_payload["data"]["itemStatus"], "0");
    assert_eq!(open_payload["data"]["itemId"], "item-123");

    let captured = upstream.requests.lock().unwrap().clone();

    let kgraph_request = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.taobao.idle.kgraph.property.recommend/2.0/")
        .unwrap();
    assert_eq!(
        kgraph_request.query.get("api").map(String::as_str),
        Some("mtop.taobao.idle.kgraph.property.recommend")
    );
    assert_eq!(kgraph_request.data["title"], "Live Chair");

    let publish_request = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.idle.wx.idleitem.publish/1.0/")
        .unwrap();
    assert_eq!(publish_request.data["itemTextDTO"]["title"], "Live Chair");
    assert_eq!(publish_request.data["itemCatDTO"]["channelCatId"], "12345");
    assert_eq!(publish_request.data["itemAddrDTO"]["divisionId"], 330106);
    assert_eq!(publish_request.data["simpleItem"], false);

    let wx_edit_request = captured
        .iter()
        .find(|request| request.path == "/wx/h5/mtop.idle.wx.idleitem.edit/1.0/")
        .unwrap();
    assert_eq!(wx_edit_request.data["itemId"], "item-123");
    assert_eq!(
        wx_edit_request.data["itemTextDTO"]["title"],
        "Edited Live Chair"
    );

    let h5_edit_request = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.idle.idleitem.edit/1.0/")
        .unwrap();
    assert_eq!(h5_edit_request.data["itemId"], "item-123");
    assert_eq!(
        h5_edit_request.data["itemTextDTO"]["title"],
        "Edited Live Chair"
    );
    assert_eq!(h5_edit_request.data["itemPriceDTO"]["priceInCent"], "9900");
    assert_eq!(h5_edit_request.data["itemPriceDTO"]["noBargain"], true);

    let open_request = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.idle.idleitem.publish/1.0/")
        .unwrap();
    assert_eq!(open_request.data["itemId"], "item-123");
    assert_eq!(open_request.data["itemStatus"], "0");
    assert_eq!(open_request.data["itemAddrDTO"]["divisionId"], "330106");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn publish_route_defaults_simple_item_to_false_when_omitted() {
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

    let publish_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/publish")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-builder",
                        "title": "测试服务商品",
                        "desc": "测试服务商品描述",
                        "price": "88.00",
                        "image_urls": ["https://img.alicdn.com/service.jpg"],
                        "division_id": "330106",
                        "city": "杭州市",
                        "province": "浙江省",
                        "area": "西湖区",
                        "latitude": "30.2741",
                        "longitude": "120.1551"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(publish_response.status(), StatusCode::OK);
    let publish_payload = json_body(publish_response).await;
    assert_eq!(publish_payload["data"]["itemId"], "new-live-item");

    let captured = upstream.requests.lock().unwrap().clone();
    let publish_request = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.idle.wx.idleitem.publish/1.0/")
        .unwrap();
    assert_eq!(publish_request.data["simpleItem"], false);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn coin_deduction_routes_follow_upstream_and_do_not_fake_success() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", format!("{base_url}/wx"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_account(&accounts_pool).await;
    seed_cached_item(&xianyu_pool, "uid-builder", "item-ok").await;
    seed_cached_item(&xianyu_pool, "uid-builder", "item-fail").await;

    let open_ok_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/coin/deduction/open")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-builder",
                        "item_ids": ["item-ok"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(open_ok_response.status(), StatusCode::OK);
    let open_ok_payload = json_body(open_ok_response).await;
    assert_eq!(open_ok_payload["ret"][0], "SUCCESS::调用成功");

    let ok_coin_open: i64 =
        sqlx::query_scalar("SELECT coin_open FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-builder")
            .bind("item-ok")
            .fetch_one(&xianyu_pool)
            .await
            .unwrap();
    let ok_coin_ratio: String =
        sqlx::query_scalar("SELECT coin_ratio FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-builder")
            .bind("item-ok")
            .fetch_one(&xianyu_pool)
            .await
            .unwrap();
    assert_eq!(ok_coin_open, 1);
    assert_eq!(ok_coin_ratio, "20");

    let open_fail_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/coin/deduction/open")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-builder",
                        "item_ids": ["item-fail"],
                        "ratio": "30"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(open_fail_response.status(), StatusCode::OK);
    let open_fail_payload = json_body(open_fail_response).await;
    assert_eq!(
        open_fail_payload["ret"][0],
        "FAIL_SYS_UNAUTHORIZED_ENTRANCE::API访问入口未授权"
    );

    let fail_coin_open: i64 =
        sqlx::query_scalar("SELECT coin_open FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-builder")
            .bind("item-fail")
            .fetch_one(&xianyu_pool)
            .await
            .unwrap();
    let fail_coin_ratio: String =
        sqlx::query_scalar("SELECT coin_ratio FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-builder")
            .bind("item-fail")
            .fetch_one(&xianyu_pool)
            .await
            .unwrap();
    assert_eq!(fail_coin_open, 0);
    assert_eq!(fail_coin_ratio, "");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn kgraph_route_derives_upstream_title_from_description_when_title_missing() {
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

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/item/kgraph")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-builder",
                        "title": "",
                        "desc": "二手 iPhone 15 Pro Max，白色 256G，国行在保"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let captured = upstream.requests.lock().unwrap().clone();
    let kgraph_requests = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.taobao.idle.kgraph.property.recommend/2.0/")
        .collect::<Vec<_>>();
    assert!(
        !kgraph_requests.is_empty(),
        "should capture at least one kgraph upstream request"
    );
    for request in kgraph_requests {
        assert_eq!(request.data["title"], "二手 iPhone 15 Pro Max");
        assert_eq!(
            request.data["description"],
            "二手 iPhone 15 Pro Max，白色 256G，国行在保"
        );
    }

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
    Query(query): Query<HashMap<String, String>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    record_request(&state, path.clone(), query, &body);

    match path.as_str() {
        "/h5/mtop.taobao.idle.kgraph.property.recommend/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "cardList": [
                    {
                        "cardData": {
                            "propertyId": "-10000",
                            "propertyName": "分类",
                            "valuesList": [
                                {
                                    "catId": "50012010",
                                    "tbCatId": "50012010",
                                    "channelCatId": "12345",
                                    "catName": "椅子",
                                    "leafId": "60001"
                                }
                            ]
                        }
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.idle.idleitem.edit/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "updated": true,
                "itemId": "item-123"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.pc.detail/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "itemDO": {
                    "itemId": "item-123",
                    "title": "Sold Live Chair",
                    "desc": "Sold wood chair",
                    "soldPrice": "9900",
                    "originalPrice": "19900",
                    "imageInfos": [
                        { "url": "https://img.alicdn.com/source-chair.jpg" }
                    ]
                }
            }
        }))
        .into_response(),
        "/h5/mtop.idle.idleitem.publish/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "itemId": "item-123",
                "itemStatus": "0"
            }
        }))
        .into_response(),
        "/h5/mtop.idle.wx.idleitem.publish/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "itemId": "new-live-item"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.deduction.new.open/1.0/" => {
            let item_id_list = extract_string(&extract_data_json(&body), "itemIdList");
            if item_id_list.contains("item-fail") {
                axum::Json(json!({
                    "ret": ["FAIL_SYS_UNAUTHORIZED_ENTRANCE::API访问入口未授权"]
                }))
                .into_response()
            } else {
                axum::Json(json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": {
                        "success": true
                    }
                }))
                .into_response()
            }
        }
        "/h5/mtop.taobao.idle.coin.deduction.new.close/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "success": true
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

async fn wx_handler(
    State(state): State<UpstreamState>,
    _headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    record_request(&state, path.clone(), query, &body);

    match path.as_str() {
        "/wx/h5/mtop.idle.wx.idleitem.publish/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "itemId": "new-live-item"
            }
        }))
        .into_response(),
        "/wx/h5/mtop.idle.wx.idleitem.edit/1.0/" => axum::Json(json!({
            "ret": ["FAIL_BIZ_WECHAT_NOT_SUPPORT_PUBLISH_OR_EDIT::wx not support"]
        }))
        .into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({ "ret": ["FAIL_SYS_NOT_FOUND::missing"] })),
        )
            .into_response(),
    }
}

fn record_request(
    state: &UpstreamState,
    path: String,
    query: HashMap<String, String>,
    raw_body: &[u8],
) {
    state.requests.lock().unwrap().push(CapturedRequest {
        path,
        data: extract_data_json(raw_body),
        query,
    });
}

fn extract_data_json(raw_body: &[u8]) -> Value {
    let decoded = percent_decode(raw_body);
    let data = decoded.strip_prefix("data=").unwrap_or(decoded.as_str());
    serde_json::from_str(data).unwrap_or(Value::Null)
}

fn extract_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn percent_decode(raw: &[u8]) -> String {
    let mut output = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        match raw[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < raw.len() => {
                let hi = hex_value(raw[index + 1]);
                let lo = hex_value(raw[index + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    output.push((hi << 4) | lo);
                    index += 3;
                } else {
                    output.push(raw[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
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

fn with_auth(mut request: Request<Body>) -> Request<Body> {
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {TEST_BEARER}")).unwrap(),
    );
    request
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
    .bind("uid-builder")
    .bind("Builder")
    .bind("seed-builder-token_1710000000000")
    .bind("sid-builder")
    .bind(
        json!({
            "_m_h5_tk": "seed-builder-token_1710000000000",
            "_m_h5_tk_enc": "seed-builder-token-enc",
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

async fn seed_cached_item(pool: &SqlitePool, uid: &str, item_id: &str) {
    sqlx::query(
        r#"
        INSERT INTO account_items (item_id, uid, title, created_at, updated_at)
        VALUES (?, ?, ?, 0, 0)
        "#,
    )
    .bind(item_id)
    .bind(uid)
    .bind("Seed item")
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
