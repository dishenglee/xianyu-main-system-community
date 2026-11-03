use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    routing::post,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "m020-datacompass";

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    data: Value,
}

#[tokio::test]
async fn datacompass_single_routes_forward_h5_and_add_normalized_data() {
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

    let response = app
        .oneshot(
            Request::get(
                "/api/shop/datacompass/items?uid=uid-upstream&dateType=recent7d&page=2&pageSize=10",
            )
            .header("authorization", format!("Bearer {TEST_BEARER}"))
            .body(axum::body::Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["items"][0]["itemId"], "item-1");
    assert_eq!(
        payload["data"]["normalized"]["realDateRange"],
        json!(["20260526", "20260526"])
    );
    assert!(payload["data"]["normalized"]["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|metric| metric["name"] == "payAmt" && metric["label"] == "支付金额"));

    let captured = upstream.requests.lock().unwrap().clone();
    let request = captured
        .iter()
        .find(|request| {
            request.path == "/h5/mtop.alibaba.idle.seller.app.datacompass.item.list/1.0/"
        })
        .expect("item list request should reach H5 upstream");
    assert_eq!(request.data["dateType"], "recent7d");
    assert_eq!(request.data["page"], "2");
    assert_eq!(request.data["pageSize"], "10");
    assert_eq!(request.data["fishAdCode"], "330106");
    assert_eq!(request.data["gps"], "30.2741,120.1551");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn datacompass_summary_fans_and_cs_bundle_expected_apis() {
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

    for path in [
        "/api/shop/datacompass/summary?uid=uid-upstream&dateType=recent30d",
        "/api/shop/datacompass/fans?uid=uid-upstream&dateType=recent30d",
        "/api/shop/datacompass/cs?uid=uid-upstream&dateType=recent30d",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header("authorization", format!("Bearer {TEST_BEARER}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let payload = json_body(response).await;
        assert_eq!(payload["ret"][0], "SUCCESS::调用成功");
        assert!(payload["data"]["normalized"]["metrics"].is_array());
    }

    let captured = upstream.requests.lock().unwrap().clone();
    for path in [
        "/h5/mtop.alibaba.idle.seller.app.datacompass.singleuser.seller.summary/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.singleuser.browse.summary/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.singleuser.item.summary/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.singleuser.repurchase.summary/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.fans.summary/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.fans.insights.query/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.cs.overview.summary/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.cs.detail.list/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.cs.evaluation.list/1.0/",
    ] {
        assert!(
            captured.iter().any(|request| request.path == path),
            "missing H5 path {path}"
        );
    }
    assert!(captured
        .iter()
        .all(|request| request.data["dateType"] == "recent30d"));

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn datacompass_basic_routes_cover_gray_begin_time_merchant_flow_indicators_refund() {
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

    for path in [
        "/api/shop/datacompass/gray?uid=uid-upstream",
        "/api/shop/datacompass/begin-time?uid=uid-upstream",
        "/api/shop/datacompass/merchant?uid=uid-upstream",
        "/api/shop/datacompass/flow?uid=uid-upstream",
        "/api/shop/datacompass/item-indicators?uid=uid-upstream&itemId=item-1",
        "/api/shop/datacompass/refund?uid=uid-upstream",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header("authorization", format!("Bearer {TEST_BEARER}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let payload = json_body(response).await;
        assert!(
            payload["data"]["normalized"]["metrics"].is_array(),
            "{path}"
        );
    }

    let captured = upstream.requests.lock().unwrap().clone();
    for path in [
        "/h5/mtop.alibaba.idle.seller.common.query.user.gray.info/1.0/",
        "/h5/mtop.alibaba.idle.seller.datacompass.get.begin.time/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.merchant.info.query/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.flow.detail/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.item.indicators/1.0/",
        "/h5/mtop.alibaba.idle.seller.app.datacompass.refund.summary/1.0/",
    ] {
        assert!(
            captured.iter().any(|request| request.path == path),
            "missing H5 path {path}"
        );
    }

    let _ = shutdown_tx.send(());
}

async fn spawn_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{addr}"), shutdown_tx)
}

async fn h5_handler(
    State(state): State<UpstreamState>,
    Path((api, version)): Path<(String, String)>,
    Query(_query): Query<HashMap<String, String>>,
    body: Bytes,
) -> Json<Value> {
    let path = format!("/h5/{api}/{version}/");
    let data = parse_data_body(&body);
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        data,
    });
    Json(match path.as_str() {
        "/h5/mtop.alibaba.idle.seller.app.datacompass.item.list/1.0/" => json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "realDate": "20260526",
                "payAmt": 12.5,
                "items": [{ "itemId": "item-1", "title": "露营车", "payAmt": 12.5 }]
            }
        }),
        _ => json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "realStartDate": "20260526",
                "realEndDate": "20260526",
                "payAmt": 0,
                "visitorCnt": 3,
                "cards": [
                    { "name": "browseCnt", "label": "浏览量", "value": 8, "display": "8", "ratio": "1%" }
                ],
                "api": api
            }
        }),
    })
}

fn parse_data_body(body: &[u8]) -> Value {
    let raw = String::from_utf8_lossy(body);
    let encoded = raw
        .split('&')
        .find_map(|part| part.strip_prefix("data="))
        .unwrap_or_default();
    let decoded = urlencoding::decode(encoded).unwrap();
    serde_json::from_str(&decoded).unwrap()
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
    .bind("uid-upstream")
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

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
