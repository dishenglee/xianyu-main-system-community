use std::{
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    extract::{Request as AxumRequest, State},
    http::{HeaderMap, Request, StatusCode},
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
    license_runtime::{set_sign_license_context, SignLicenseContext},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    cookie: String,
    body: String,
    headers: Vec<(String, String)>,
}

#[tokio::test]
async fn account_summary_force_fetches_real_upstream_and_persists_metrics() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", format!("{base_url}/wx"));
    env::set_var("XIANYU_SIGN_SERVICE_URL", &base_url);
    env::set_var("XIANYU_LICENSE_SERVER_URL", &base_url);
    env::set_var("XIANYU_APP_MTOP_GATEWAY", format!("{base_url}/gw"));
    set_sign_license_context(Some(SignLicenseContext {
        server_url: base_url.clone(),
        auth_token: "auth-token".to_string(),
        signed_token: "signed-token".to_string(),
        license_id: 7,
        username: "test-user".to_string(),
        sku: "trial".to_string(),
        account_limit: 10,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-1".to_string(),
    }));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(
            Request::get("/api/account/summary?uid=uid-upstream&force=true")
                .header("authorization", "Bearer test-api-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["securityScore"]["totalScore"], 88);
    assert_eq!(payload["coinBalance"], 123);
    assert_eq!(payload["violations"]["totalCount"], 2);
    assert_eq!(payload["scoreRecords"].as_array().map(Vec::len), Some(2));
    assert_eq!(payload["fishPower"], 785);
    assert_eq!(payload["bill"]["monthAmount"], "888.00");
    assert!(payload.get("trade").is_none());
    assert_eq!(payload["fishShopEnabled"], true);
    assert_eq!(payload["fishShopLevel"], "Lv.3");
    assert_eq!(payload["seller30d"]["已卖出"]["value"], "666.00");
    assert!((payload["totalEarned"].as_f64().unwrap() - 34670.78).abs() < 0.001);

    let row = sqlx::query(
        "SELECT security_score, coin_balance, violation_count, fish_power, seller_30d_sold, extra FROM accounts WHERE uid = ?",
    )
    .bind("uid-upstream")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(row.get::<i64, _>("security_score"), 88);
    assert_eq!(row.get::<i64, _>("coin_balance"), 123);
    assert_eq!(row.get::<i64, _>("violation_count"), 2);
    assert_eq!(row.get::<i64, _>("fish_power"), 785);
    assert_eq!(row.get::<String, _>("seller_30d_sold"), "666.00");
    assert!(row.get::<String, _>("extra").contains("\"summary_cache\""));
    assert!(row
        .get::<String, _>("extra")
        .contains("\"totalEarned\":34670.78"));

    let captured = upstream.requests.lock().unwrap().clone();
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.aq2.security.score.get/1.0/"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idle.coin.activity.user.info/1.0/"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.idle.security.violation.list/1.0/"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idle.bill.list/1.0/"));
    assert!(captured.iter().any(|request| request.path
        == "/h5/mtop.alibaba.idle.seller.app.datacompass.singleuser.seller.summary/1.0/"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/api/licenses/device-key/register"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/api/licenses/runtime-challenge"));
    assert!(captured.iter().any(|request| request.path == "/sign"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/api/me/xianyu-account-bindings/resolve"));
    assert!(captured
        .iter()
        .filter(|request| {
            request.path == "/api/licenses/runtime-challenge" || request.path == "/sign"
        })
        .all(|request| has_device_proof_headers(&request.headers)));
    assert!(captured
        .iter()
        .any(|request| request.path == "/gw/mtop.idle.user.page.my.adapter/1.0/"));
    assert!(captured
        .iter()
        .find(|request| request.path == "/gw/mtop.idle.user.page.head/1.0/")
        .map(|request| request.body.contains("uid-upstream"))
        .unwrap_or(false));
    assert!(captured
        .iter()
        .filter(|request| {
            request.path != "/api/licenses/device-key/register"
                && request.path != "/api/licenses/runtime-challenge"
                && request.path != "/sign"
                && request.path != "/api/me/xianyu-account-bindings/resolve"
        },)
        .all(|request| request.cookie.contains("uid-upstream")));

    let _ = shutdown_tx.send(());
    set_sign_license_context(None);
    clear_env();
}

#[tokio::test]
async fn account_info_routes_proxy_real_upstream_payloads() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_WX_GATEWAY", format!("{base_url}/wx"));
    env::set_var("XIANYU_SIGN_SERVICE_URL", &base_url);
    env::set_var("XIANYU_LICENSE_SERVER_URL", &base_url);
    env::set_var("XIANYU_APP_MTOP_GATEWAY", format!("{base_url}/gw"));
    set_sign_license_context(Some(SignLicenseContext {
        server_url: base_url.clone(),
        auth_token: "auth-token".to_string(),
        signed_token: "signed-token".to_string(),
        license_id: 7,
        username: "test-user".to_string(),
        sku: "trial".to_string(),
        account_limit: 10,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-1".to_string(),
    }));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let security_score =
        request_json(app.clone(), "/api/account/security_score?uid=uid-upstream").await;
    assert_eq!(security_score["data"]["data"]["totalScore"], 88);

    let security_score_records = request_json(
        app.clone(),
        "/api/account/security_score/records?uid=uid-upstream&page=1",
    )
    .await;
    assert_eq!(
        security_score_records["data"]["data"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    let violations = request_json(app.clone(), "/api/account/violations?uid=uid-upstream").await;
    assert_eq!(violations["data"]["totalCount"], 2);
    assert_eq!(violations["data"]["data"][0]["recordId"], "viol-1");

    let violations_all = request_json(
        app.clone(),
        "/api/account/violations/all?uid=uid-upstream&page_size=50",
    )
    .await;
    assert_eq!(violations_all["data"]["totalCount"], 3);
    assert_eq!(violations_all["data"]["data"][2]["recordId"], "viol-3");

    let security_detection = request_json(
        app.clone(),
        "/api/account/security_detection?uid=uid-upstream",
    )
    .await;
    assert_eq!(security_detection["data"]["data"][0]["title"], "设备风险");

    let qualification =
        request_json(app.clone(), "/api/account/qualification?uid=uid-upstream").await;
    assert_eq!(qualification["data"]["data"]["realName"], true);

    let coin_balance =
        request_json(app.clone(), "/api/account/coin_balance?uid=uid-upstream").await;
    assert_eq!(coin_balance["data"]["myIdleCoin"], 123);

    let coin_detail = request_json(
        app.clone(),
        "/api/account/coin_detail?uid=uid-upstream&page=1&page_size=20",
    )
    .await;
    assert_eq!(
        coin_detail["data"]["userScoreDetailDOList"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );

    let coin_account = request_json(app.clone(), "/api/coin/account?uid=uid-upstream").await;
    assert_eq!(coin_account["data"]["availableBalance"], 123);

    let seller_data = request_json(
        app.clone(),
        "/api/account/seller_data?uid=uid-upstream&date_type=recent30d",
    )
    .await;
    assert_eq!(
        seller_data["data"]["data"]["graphBannerBenchData"]["bannerDataList"][0]["dataStr"],
        "666.00"
    );

    let user_page = request_json(app.clone(), "/api/account/user_page?uid=uid-upstream").await;
    assert_eq!(
        user_page["data"]["module"]["base"]["creditScore"]["score"],
        66
    );

    let user_head = request_json(app.clone(), "/api/account/user_head?uid=uid-upstream").await;
    assert_eq!(
        user_head["data"]["module"]["base"]["displayName"],
        "Seller Upstream"
    );

    let user_check = request_json(app.clone(), "/api/account/user_check?uid=uid-upstream").await;
    assert_eq!(user_check["data"]["status"], "active");

    let alipay = request_json(app.clone(), "/api/account/alipay?uid=uid-upstream").await;
    assert_eq!(alipay["data"]["bound"], true);

    let profile = request_json(app.clone(), "/api/account/profile?uid=uid-upstream").await;
    assert_eq!(profile["securityScore"]["totalScore"], 88);
    assert_eq!(profile["fishPower"], 785);
    assert_eq!(profile["fishShopEnabled"], true);
    assert_eq!(profile["fishShopLevel"], "Lv.3");

    let _ = shutdown_tx.send(());
    set_sign_license_context(None);
    clear_env();
}

#[tokio::test]
async fn account_profile_post_updates_real_upstream_with_location_payload() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_SIGN_SERVICE_URL", &base_url);
    env::set_var("XIANYU_LICENSE_SERVER_URL", &base_url);
    set_sign_license_context(Some(SignLicenseContext {
        server_url: base_url.clone(),
        auth_token: "auth-token".to_string(),
        signed_token: "signed-token".to_string(),
        license_id: 7,
        username: "test-user".to_string(),
        sku: "trial".to_string(),
        account_limit: 10,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-1".to_string(),
    }));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/account/profile")
                .header("authorization", "Bearer test-api-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "signature": "新的个性签名",
                        "signatureTitle": "签名标题"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["raw"]["data"]["updated"], true);
    assert_eq!(payload["signature"], "新的个性签名");
    assert_eq!(payload["signatureTitle"], "签名标题");

    let captured = upstream.requests.lock().unwrap().clone();
    let update = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.taobao.idle.user.info.update/1.0/")
        .expect("profile update upstream request");
    assert!(update.body.contains("signature"));
    assert!(update
        .body
        .contains("%E6%96%B0%E7%9A%84%E4%B8%AA%E6%80%A7%E7%AD%BE%E5%90%8D"));
    assert!(update.body.contains("signatureTitle"));
    assert!(update.body.contains("%E7%AD%BE%E5%90%8D%E6%A0%87%E9%A2%98"));
    assert!(update.body.contains("fishAdCode"));
    assert!(update.body.contains("330106"));
    assert!(update.body.contains("gps"));
    assert!(update.body.contains("30.2741%2C120.1551"));

    let _ = shutdown_tx.send(());
    set_sign_license_context(None);
    clear_env();
}

#[tokio::test]
async fn account_profile_post_keeps_explicit_empty_profile_fields() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_SIGN_SERVICE_URL", &base_url);
    env::set_var("XIANYU_LICENSE_SERVER_URL", &base_url);
    set_sign_license_context(Some(SignLicenseContext {
        server_url: base_url.clone(),
        auth_token: "auth-token".to_string(),
        signed_token: "signed-token".to_string(),
        license_id: 7,
        username: "test-user".to_string(),
        sku: "trial".to_string(),
        account_limit: 10,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-1".to_string(),
    }));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/account/profile")
                .header("authorization", "Bearer test-api-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "signature": "",
                        "signatureTitle": "",
                        "defaultNick": false,
                        "gender": 0
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["signature"], "");
    assert_eq!(payload["signatureTitle"], "");
    assert_eq!(payload["defaultNick"], "false");
    assert_eq!(payload["gender"], "0");

    let captured = upstream.requests.lock().unwrap().clone();
    let update = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.taobao.idle.user.info.update/1.0/")
        .expect("profile update upstream request");
    let decoded_body = urlencoding::decode(&update.body).unwrap();
    assert!(decoded_body.contains("\"signature\":\"\""));
    assert!(decoded_body.contains("\"signatureTitle\":\"\""));
    assert!(decoded_body.contains("\"defaultNick\":\"false\""));
    assert!(decoded_body.contains("\"gender\":\"0\""));

    let _ = shutdown_tx.send(());
    set_sign_license_context(None);
    clear_env();
}

#[tokio::test]
async fn account_profile_post_marks_missing_upstream_ret_as_failure() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));
    env::set_var("XIANYU_SIGN_SERVICE_URL", &base_url);
    env::set_var("XIANYU_LICENSE_SERVER_URL", &base_url);
    set_sign_license_context(Some(SignLicenseContext {
        server_url: base_url.clone(),
        auth_token: "auth-token".to_string(),
        signed_token: "signed-token".to_string(),
        license_id: 7,
        username: "test-user".to_string(),
        sku: "trial".to_string(),
        account_limit: 10,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-1".to_string(),
    }));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/account/profile")
                .header("authorization", "Bearer test-api-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-upstream",
                        "signature": "retless-probe"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["ret"][0], "FAIL_SYS_EMPTY_RET::上游未返回 ret");
    assert!(payload["raw"].get("ret").is_none());

    let _ = shutdown_tx.send(());
    set_sign_license_context(None);
    clear_env();
}

async fn spawn_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler))
        .route("/wx/h5/:api/:version/", post(wx_handler))
        .route(
            "/api/licenses/device-key/register",
            post(device_key_register_handler),
        )
        .route(
            "/api/licenses/runtime-challenge",
            post(runtime_challenge_handler),
        )
        .route(
            "/api/me/xianyu-account-bindings/resolve",
            post(resolve_binding_handler),
        )
        .route("/sign", post(sign_service_handler))
        .route("/gw/:api/:version/", post(app_mtop_handler))
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
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&body).to_string();
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        cookie,
        body: body.clone(),
        headers: header_pairs(&headers),
    });

    match path.as_str() {
        "/h5/mtop.taobao.aq2.security.score.get/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "data": {
                    "totalScore": 88,
                    "gmtModified": "2026-04-17 10:00:00",
                    "lastChangedScoreRecord": {
                        "changeTypeName": "加分",
                        "realChangeScore": 3,
                        "decrease": false,
                        "scoreAfter": 88,
                        "scoreBefore": 85,
                        "gmtCreate": "2026-04-16 10:00:00"
                    }
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.aq2.score.record.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "data": [
                    {
                        "changeTypeName": "加分",
                        "realChangeScore": 3,
                        "decrease": false,
                        "scoreAfter": 88,
                        "scoreBefore": 85,
                        "gmtCreate": "2026-04-16 10:00:00"
                    },
                    {
                        "changeTypeName": "减分",
                        "realChangeScore": 2,
                        "decrease": true,
                        "scoreAfter": 85,
                        "scoreBefore": 87,
                        "gmtCreate": "2026-04-15 10:00:00"
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.idle.security.violation.list/1.0/" => {
            let payload = if body.contains("in90Days%22%3Afalse") {
                json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": {
                        "totalCount": 3,
                        "hasNext": false,
                        "data": [
                            { "recordId": "viol-1" },
                            { "recordId": "viol-2" },
                            { "recordId": "viol-3" }
                        ]
                    }
                })
            } else {
                json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": {
                        "totalCount": 2,
                        "hasNext": true,
                        "data": [
                            { "recordId": "viol-1" },
                            { "recordId": "viol-2" }
                        ]
                    }
                })
            };
            axum::Json(payload).into_response()
        }
        "/h5/mtop.taobao.aq2.detection.result.get/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "data": [
                    { "title": "设备风险", "status": "safe" }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.aq2.qualification.get/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "data": {
                    "realName": true,
                    "zhima": true
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.activity.user.info/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "myIdleCoin": 123
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.user.detail.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "userScoreDetailDOList": [
                    { "bizType": "签到", "score": 10 },
                    { "bizType": "发布", "score": 20 }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.point.pointbank.accountinfo/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "balance": 123,
                "totalBalance": 123,
                "coinBalance": 123,
                "availableBalance": 123
            }
        }))
        .into_response(),
        "/h5/mtop.alibaba.idle.seller.app.datacompass.singleuser.seller.summary/1.0/" => {
            axum::Json(json!({
                "ret": ["SUCCESS::调用成功"],
                "data": {
                    "data": {
                        "graphBannerBenchData": {
                            "bannerDataList": [
                                {
                                    "name": "已卖出",
                                    "dataStr": "666.00",
                                    "cycle": "recent30d",
                                    "ratioFormat": "+12%"
                                }
                            ]
                        }
                    }
                }
            }))
            .into_response()
        }
        "/h5/mtop.taobao.idle.user.check/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "status": "active",
                "uid": "uid-upstream"
            }
        }))
        .into_response(),
        "/h5/com.taobao.idle.user.alipay/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "bound": true,
                "maskedAccount": "a***@example.com"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.bill.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "monthAmount": "888.00",
                "monthOrderCount": 9
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.user.info.update/1.0/" => {
            let decoded_body = urlencoding::decode(&body)
                .map(|value| value.into_owned())
                .unwrap_or_else(|_| body.clone());
            let payload = if decoded_body.contains("retless-probe") {
                json!({
                    "data": {
                        "updated": true
                    }
                })
            } else {
                json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": {
                        "updated": true
                    }
                })
            };
            axum::Json(payload).into_response()
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
    headers: HeaderMap,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    let _body = String::from_utf8_lossy(&body).to_string();
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        cookie,
        body: _body,
        headers: header_pairs(&headers),
    });

    match path.as_str() {
        "/wx/h5/mtop.idle.wx.user.page.my/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "module": {
                    "base": {
                        "creditScore": {
                            "score": 66
                        }
                    },
                    "trade": {
                        "buyerOrderCount": 1,
                        "sellerOrderCount": 2,
                        "itemCount": 3
                    }
                }
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

async fn device_key_register_handler(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let bytes = request.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path,
        cookie: String::new(),
        body,
        headers: header_pairs(&headers),
    });
    axum::Json(json!({
        "registered": true
    }))
    .into_response()
}

async fn runtime_challenge_handler(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let bytes = request.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path,
        cookie: body,
        body: String::new(),
        headers: header_pairs(&headers),
    });
    axum::Json(json!({
        "challenge_id": "challenge-123"
    }))
    .into_response()
}

async fn sign_service_handler(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let bytes = request.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path,
        cookie: body,
        body: String::new(),
        headers: header_pairs(&headers),
    });

    axum::Json(json!({
        "signHeaders": {
            "x-sign": "sign-value",
            "x-sgext": "sgext-value",
            "x-mini-wua": "mini-wua-value",
            "x-umt": "umt-value"
        },
        "updatedInput": {
            "t": "1770000000"
        },
        "requestHeaders": {
            "x-appkey": "258",
            "x-t": "1770000000"
        }
    }))
    .into_response()
}

async fn resolve_binding_handler(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let bytes = request.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path,
        cookie: String::new(),
        body: body.clone(),
        headers: header_pairs(&headers),
    });
    let payload: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
    let rows = payload
        .get("account_uids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|uid| {
            json!({
                "account_uid": uid,
                "status": "owned_by_me",
                "owner_username": "test-user",
                "message": ""
            })
        })
        .collect::<Vec<_>>();
    axum::Json(Value::Array(rows)).into_response()
}

async fn app_mtop_handler(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let bytes = request.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes).to_string();
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        cookie,
        body,
        headers: header_pairs(&headers),
    });

    match path.as_str() {
        "/gw/mtop.idle.user.page.head/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "module": {
                    "base": {
                        "displayName": "Seller Upstream",
                        "creditLevel": 5,
                        "shop": {
                            "level": "L3",
                            "levelPic": "https://img.example.com/fish-shop-L3.png"
                        }
                    }
                }
            }
        }))
        .into_response(),
        "/gw/mtop.idle.user.page.my.adapter/1.0/" => axum::Json(json!({
            "ret": "SUCCESS",
            "data": {
                "base": {
                    "cs": {
                        "score": 785
                    }
                },
                "container": {
                    "sections": [
                        {
                            "sectionBizCode": "trade2026",
                            "item": {
                                "top": {
                                    "exContent": {
                                        "income": "3.46万",
                                        "sold": "131",
                                        "title": "我的交易",
                                        "subTitle": "在闲鱼赚了34670.78元"
                                    },
                                    "itemBizCode": "top",
                                    "valid": "true"
                                }
                            }
                        }
                    ]
                }
            }
        }))
        .into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({ "ret": "FAIL_SYS_NOT_FOUND", "message": "missing" })),
        )
            .into_response(),
    }
}

async fn request_json(app: Router, uri: &str) -> Value {
    let response = app
        .oneshot(
            Request::get(uri)
                .header("authorization", "Bearer test-api-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
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
            owner_username,
            extra,
            security_score,
            security_score_updated_at,
            restriction_end,
            coin_balance,
            violation_count,
            fish_power,
            seller_30d_sold
        )
        VALUES (?, ?, ?, ?, '', ?, '[]', '', 'active', 0, 0, 0, 0, 0, 0, '', '', '', ?, ?, -1, 0, '', 0, 0, 0, '')
        "#,
    )
    .bind("uid-upstream")
    .bind("Seller Upstream")
    .bind("seed-token_1700000000000")
    .bind("seed-cookie2")
    .bind(
        json!({
            "_m_h5_tk": "seed-token_1700000000000",
            "_m_h5_tk_enc": "seed-enc",
            "cookie2": "seed-cookie2",
            "unb": "uid-upstream",
            "tracknick": "Seller Upstream"
        })
        .to_string(),
    )
    .bind("test-user")
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

fn header_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn has_device_proof_headers(headers: &[(String, String)]) -> bool {
    let names = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    names.contains(&"x-device-public-key")
        && names.contains(&"x-device-proof-ts")
        && names.contains(&"x-device-proof-nonce")
        && names.contains(&"x-device-proof-signature")
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var(
        "XIANYU_SHARED_DEVICE_KEY_DIR",
        data_dir.join("device-key-shared").display().to_string(),
    );
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::set_var("API_TOKEN", "test-api-token");
}

fn clear_env() {
    set_sign_license_context(None);
    env::remove_var("APP_DATA_DIR");
    env::remove_var("JWT_SECRET");
    env::remove_var("ADMIN_USERNAME");
    env::remove_var("ADMIN_PASSWORD");
    env::remove_var("API_TOKEN");
    env::remove_var("XIANYU_H5_GATEWAY");
    env::remove_var("XIANYU_WX_GATEWAY");
    env::remove_var("XIANYU_SIGN_SERVICE_URL");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    env::remove_var("XIANYU_APP_MTOP_GATEWAY");
    env::remove_var("XIANYU_SHARED_DEVICE_KEY_DIR");
}
