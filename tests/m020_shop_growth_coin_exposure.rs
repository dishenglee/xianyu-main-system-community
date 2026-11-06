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
use xianyu_rust_backend_full::{
    app::build_app,
    config::AppConfig,
    license_runtime::{get_sign_license_context, set_sign_license_context, SignLicenseContext},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "legacy-token";

#[test]
fn promotion_growth_routes_are_alias_layer_not_duplicate_business_handlers() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let promotion_source =
        std::fs::read_to_string(format!("{manifest_dir}/src/routes/market/promotion.rs")).unwrap();

    assert!(promotion_source.contains("coin::promotion_alias_routes()"));
    assert!(promotion_source.contains("exposure::promotion_alias_routes()"));
    assert!(!promotion_source.contains("mtop.taobao.idle.coin.promotion"));
    assert!(!promotion_source.contains("mtop.alibaba.idle.shop.incentive.task"));
    assert!(!promotion_source.contains("fn coin_promotion_create_data"));
    assert!(!promotion_source.contains("fn extract_exposure_tasks"));
}

#[derive(Clone)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    task_list_calls: Arc<Mutex<usize>>,
    coin_task_list_calls: Arc<Mutex<usize>>,
    real_exposure_shapes: bool,
    dynamic_exposure_shapes: bool,
    real_coin_view_shapes: bool,
    force_app_super_polish_account_fail: bool,
    force_h5_super_polish_trigger_fail: bool,
}

impl Default for UpstreamState {
    fn default() -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            task_list_calls: Arc::new(Mutex::new(0)),
            coin_task_list_calls: Arc::new(Mutex::new(0)),
            real_exposure_shapes: false,
            dynamic_exposure_shapes: false,
            real_coin_view_shapes: false,
            force_app_super_polish_account_fail: false,
            force_h5_super_polish_trigger_fail: false,
        }
    }
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    data: Value,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
}

#[tokio::test]
async fn promotion_read_routes_forward_captured_h5_apis_and_defaults() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_item(&xianyu_pool, "uid-growth", "1055797812835").await;

    for path in [
        "/api/promotion/coin/signin/home?uid=uid-growth",
        "/api/promotion/coin/tasks?uid=uid-growth",
        "/api/promotion/coin/packages?uid=uid-growth&itemId=item-1",
        "/api/promotion/coin/orders?uid=uid-growth&page=3&pageSize=10",
        "/api/promotion/coin/detail?uid=uid-growth&promotionId=promo-1",
        "/api/promotion/exposure/history?uid=uid-growth&page=2&pageSize=5",
        "/api/promotion/exposure/details?uid=uid-growth&instanceId=inst-1&bizCode=biz-1",
        "/api/promotion/shop/workbench?uid=uid-growth",
    ] {
        let response = app
            .clone()
            .oneshot(with_auth(Request::get(path).body(Body::empty()).unwrap()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }

    let captured = captured.lock().unwrap().clone();
    let sign_home = find_path(&captured, "/h5/mtop.taobao.idle.play.sign.in.home/1.0/");
    assert_eq!(sign_home.data["playId"], 4);

    let coin_tasks = find_path(
        &captured,
        "/h5/mtop.taobao.idle.task.queryolivertasklist/2.0/",
    );
    assert_eq!(coin_tasks.data["brand"], "redmi");
    assert_eq!(coin_tasks.data["platform"], "Android");
    assert_eq!(coin_tasks.data["sceneId"], 496);

    let packages = find_path(&captured, "/h5/mtop.taobao.idle.coin.promotion.view/2.0/");
    assert_eq!(packages.data["itemId"], "item-1");
    assert_eq!(packages.data["cardId"], "");
    assert_eq!(packages.data["needList"], true);

    let orders = find_path(&captured, "/h5/mtop.taobao.idle.coin.promotion.page/1.0/");
    assert_eq!(orders.data["pageNumber"], "3");
    assert_eq!(orders.data["pageSize"], "10");

    let detail = find_path(&captured, "/h5/mtop.taobao.idle.coin.promotion.detail/1.0/");
    assert_eq!(detail.data["id"], "promo-1");

    let history = find_path(
        &captured,
        "/h5/mtop.alibaba.idle.shop.incentive.task.exposure.history/1.0/",
    );
    assert_eq!(history.data["page"], 2);
    assert_eq!(history.data["pageNo"], 2);
    assert_eq!(history.data["pageSize"], 5);

    let details = find_path(
        &captured,
        "/h5/mtop.alibaba.idle.shop.incentive.task.exposure.details/1.0/",
    );
    assert_eq!(details.data["instanceId"], "inst-1");
    assert_eq!(details.data["bizCode"], "biz-1");

    find_path(
        &captured,
        "/h5/mtop.taobao.idle.pro.workbench.info.get/3.0/",
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn coin_aux_read_routes_forward_versions_and_payload_defaults() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    for path in [
        "/api/growth/coin/activity/user-info?uid=uid-growth",
        "/api/growth/coin/user-detail-list?uid=uid-growth&page=2&pageSize=5",
        "/api/growth/coin/virtual-card-list?uid=uid-growth",
        "/api/growth/coin/task/detail?uid=uid-growth&deliveryId=94398",
        "/api/growth/coin/task/wlj-detail?uid=uid-growth&deliveryId=94398",
        "/api/growth/coin/signin/challenge-info?uid=uid-growth",
        "/api/growth/coin/subscribe?uid=uid-growth",
    ] {
        let response = app
            .clone()
            .oneshot(with_auth(Request::get(path).body(Body::empty()).unwrap()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }

    let captured = captured.lock().unwrap().clone();

    let activity = find_path(
        &captured,
        "/h5/mtop.taobao.idle.coin.activity.user.info/1.0/",
    );
    assert!(activity.data.get("fishAdCode").is_some());
    assert!(activity.data.get("gps").is_some());

    let details = find_path(&captured, "/h5/mtop.taobao.idle.coin.user.detail.list/1.0/");
    assert_eq!(details.data["pageNumber"], "2");
    assert_eq!(details.data["pageSize"], "5");

    let cards = find_path(&captured, "/h5/mtop.taobao.idle.virtual.card.list/2.0/");
    assert_eq!(cards.data["cardType"], 7);
    assert_eq!(cards.data["pageNumber"], 1);
    assert_eq!(cards.data["pageSize"], 10);

    let task_detail = find_path(
        &captured,
        "/h5/mtop.taobao.idle.task.getolivertaskdetail/1.0/",
    );
    assert_eq!(task_detail.data["deliveryId"], "94398");
    assert_eq!(task_detail.data["sceneId"], 496);
    assert!(task_detail.data.get("fishAdCode").is_some());
    assert!(task_detail.data.get("gps").is_some());

    let wlj_detail = find_path(&captured, "/h5/mtop.taobao.idle.wlj.task.detail/1.0/");
    assert_eq!(wlj_detail.data["deliveryId"], "94398");
    assert_eq!(wlj_detail.data["sceneId"], 496);
    assert!(wlj_detail.data.get("fishAdCode").is_none());
    assert!(wlj_detail.data.get("gps").is_none());

    find_path(&captured, "/h5/mtop.idle.challenge.idleCoinSign.info/1.0/");

    let subscribe = find_path(&captured, "/h5/mtop.taobao.idle.subscribe.get/2.0/");
    assert_eq!(subscribe.data["biz"], "xianyu_coin");
    assert_eq!(subscribe.data["topic"], "sign_in");
    assert!(subscribe.data.get("fishAdCode").is_some());
    assert!(subscribe.data.get("gps").is_some());

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn growth_alias_routes_match_promotion_module_without_ad_blocked_path() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::get("/api/growth/coin/signin/home?uid=uid-growth")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let captured = captured.lock().unwrap().clone();
    find_path(&captured, "/h5/mtop.taobao.idle.play.sign.in.home/1.0/");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn promotion_write_routes_require_confirm_and_validate_expected_cost() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_item(&xianyu_pool, "uid-growth", "1055797812835").await;

    let without_confirm = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/promotion/coin/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "itemId": "1055797812835",
                        "costCoinNum": 1500,
                        "expectedCostCoinNum": 1500
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(without_confirm.status(), StatusCode::OK);
    let payload = json_body(without_confirm).await;
    assert_eq!(
        payload["ret"][0],
        "FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认"
    );

    let mismatch = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/promotion/coin/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "itemId": "1055797812835",
                        "costCoinNum": 1500,
                        "expectedCostCoinNum": 4500,
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::OK);
    let payload = json_body(mismatch).await;
    assert_eq!(
        payload["ret"][0],
        "FAIL_LOCAL_EXPECTED_COST_MISMATCH::expectedCostCoinNum 必须等于 costCoinNum"
    );
    assert!(captured.lock().unwrap().is_empty());

    let confirmed = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/promotion/coin/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "itemId": "1055797812835",
                        "costCoinNum": 1500,
                        "expectedCostCoinNum": "1500",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(confirmed.status(), StatusCode::OK);
    let payload = json_body(confirmed).await;
    assert_eq!(payload["data"]["activityId"], 827559279);

    let captured = captured.lock().unwrap().clone();
    let create = find_path(&captured, "/h5/mtop.taobao.idle.coin.promotion.create/2.0/");
    assert_eq!(create.data["itemId"], 1055797812835_i64);
    assert_eq!(create.data["costCoinNum"], 1500);
    assert!(create.data.get("expectedCostCoinNum").is_none());
    assert_eq!(create.data["promotionNum"], "50");
    assert_eq!(create.data["aimedTime"], "00:00~24:00");
    assert_eq!(create.data["asac"], "");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn promotion_create_uses_captured_view_then_create_flow_without_local_item_sync() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_item(&xianyu_pool, "uid-growth", "1055797812835").await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/promotion/coin/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "itemId": "1055797812835",
                        "promotionNum": "50",
                        "costCoinNum": 1500,
                        "expectedCostCoinNum": 1500,
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["activityId"], 827559279);

    let captured = captured.lock().unwrap().clone();
    let view = find_path(&captured, "/h5/mtop.taobao.idle.coin.promotion.view/2.0/");
    assert_eq!(view.data["itemId"], 1055797812835_i64);
    assert_eq!(view.data["cardId"], "");
    assert_eq!(view.data["needList"], true);

    let create = find_path(&captured, "/h5/mtop.taobao.idle.coin.promotion.create/2.0/");
    assert_eq!(create.data["itemId"], 1055797812835_i64);
    assert_eq!(create.data["promotionNum"], "50");
    assert_eq!(create.data["costCoinNum"], 1500);
    assert_eq!(create.data["aimedTime"], "00:00~24:00");
    assert!(create.data.get("expectedCostCoinNum").is_none());

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn promotion_exposure_safe_view_uses_captured_activity_and_benefit_shapes() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState {
        real_exposure_shapes: true,
        ..UpstreamState::default()
    };
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/promotion/exposure/tasks/complete-view")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["success"], true);

    let captured = captured.lock().unwrap().clone();
    let sync = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.sync/1.0/")
        .expect("sync request should be captured");
    assert_eq!(sync.data["activityId"], "100769");
    assert_eq!(sync.data["taskInstanceId"], "1007694526");
    assert_eq!(sync.data["complete"], true);
    assert!(sync.data.get("taskId").is_none());

    let receive = captured
        .iter()
        .find(|request| {
            request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.receive.reward/1.0/"
        })
        .expect("receive request should be captured");
    assert_eq!(receive.data["activityId"], "100769");
    assert_eq!(
        receive.data["benefitList"][0]["taskInstanceId"],
        "1007694526"
    );
    assert_eq!(
        receive.data["benefitList"][0]["rewardList"][0]["rewardType"],
        "FLOW_AMOUNT"
    );
    assert!(receive.data.get("taskId").is_none());

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn coin_complete_view_triggers_structurally_safe_coin_visit_tasks() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState {
        real_coin_view_shapes: true,
        ..UpstreamState::default()
    };
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/coin/tasks/complete-view")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["success"], true);
    assert_eq!(
        payload["data"]["triggeredTasks"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        payload["data"]["claimedRewards"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        payload["data"]["final"]["taskList"][0]["idleTaskStatus"],
        "COMPLETE"
    );

    let captured = captured.lock().unwrap().clone();
    assert!(captured
        .iter()
        .any(|request| { request.path == "/h5/mtop.taobao.idle.play.ad.event.push/1.0/" }));
    assert!(captured
        .iter()
        .any(|request| { request.path == "/h5/mtop.taobao.idle.task.getolivertaskbenefit/1.0/" }));

    assert!(!captured.iter().any(|request| {
        request.path == "/h5/mtop.taobao.idle.task.uploadOliverTaskTrigger/1.0/"
    }));

    let ad_events: Vec<_> = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.taobao.idle.play.ad.event.push/1.0/")
        .collect();
    assert_eq!(ad_events.len(), 2);
    let ad_codes = ad_events
        .iter()
        .map(|request| request.data["adPositionCode"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(ad_codes.contains(&"mmOrder_view01"));
    assert!(ad_codes.contains(&"xybAlimmss"));
    assert!(ad_events
        .iter()
        .all(|request| request.data["eventType"] == "award"));

    let task_list_calls = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.taobao.idle.task.queryolivertasklist/2.0/")
        .count();
    assert_eq!(task_list_calls, 3);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_read_routes_forward_h5_api_versions_and_query_payloads() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let tasks = app
        .clone()
        .oneshot(with_auth(
            Request::get("/api/shop/exposure/tasks?uid=uid-growth&scene=ONLY_TASK_LIST")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(tasks.status(), StatusCode::OK);

    let history = app
        .clone()
        .oneshot(with_auth(
            Request::get("/api/shop/exposure/history?uid=uid-growth&pageNo=2&pageSize=20")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(history.status(), StatusCode::OK);

    let details = app
        .clone()
        .oneshot(with_auth(
            Request::get("/api/shop/exposure/details?uid=uid-growth&itemId=item-1")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(details.status(), StatusCode::OK);

    let workbench = app
        .clone()
        .oneshot(with_auth(
            Request::get("/api/shop/workbench?uid=uid-growth")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(workbench.status(), StatusCode::OK);

    let captured = captured.lock().unwrap().clone();
    let task_request = find_path(
        &captured,
        "/h5/mtop.alibaba.idle.shop.incentive.task.list/1.0/",
    );
    assert_eq!(task_request.data["scene"], "ONLY_TASK_LIST");
    assert_eq!(
        task_request.query.get("api").map(String::as_str),
        Some("mtop.alibaba.idle.shop.incentive.task.list")
    );

    let history_request = find_path(
        &captured,
        "/h5/mtop.alibaba.idle.shop.incentive.task.exposure.history/1.0/",
    );
    assert_eq!(history_request.data["pageNo"], 2);
    assert_eq!(history_request.data["pageSize"], 20);

    let details_request = find_path(
        &captured,
        "/h5/mtop.alibaba.idle.shop.incentive.task.exposure.details/1.0/",
    );
    assert_eq!(details_request.data["itemId"], "item-1");

    find_path(
        &captured,
        "/h5/mtop.taobao.idle.pro.workbench.info.get/3.0/",
    );
    assert!(!captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idle.pro.workbench.info.get/1.0/"));

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_placement_routes_forward_real_home_and_amount_select_apis() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_item(&xianyu_pool, "uid-growth", "item-exposure-1").await;

    let home = app
        .clone()
        .oneshot(with_auth(
            Request::get("/api/growth/exposure/home?uid=uid-growth")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(home.status(), StatusCode::OK);

    let amount_options = app
        .clone()
        .oneshot(with_auth(
            Request::get("/api/growth/exposure/amount-options?uid=uid-growth")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(amount_options.status(), StatusCode::OK);

    let rendering = app
        .clone()
        .oneshot(with_auth(
            Request::get(
                "/api/growth/exposure/item-rendering?uid=uid-growth&itemId=item-exposure-1",
            )
            .body(Body::empty())
            .unwrap(),
        ))
        .await
        .unwrap();
    let rendering_status = rendering.status();
    let rendering_payload = json_body(rendering).await;
    assert_eq!(
        rendering_status,
        StatusCode::OK,
        "unexpected rendering response: {rendering_payload}"
    );

    let captured = captured.lock().unwrap().clone();
    find_path(
        &captured,
        "/h5/mtop.taobao.idle.item.exposure.coin.home/1.0/",
    );
    find_path(
        &captured,
        "/h5/mtop.taobao.idle.item.exposure.order.amount.select/1.0/",
    );
    let rendering = find_path(
        &captured,
        "/h5/mtop.taobao.idle.ad.item.activity.rendering/1.0/",
    );
    let request_json: Value =
        serde_json::from_str(rendering.data["requestJson"].as_str().unwrap()).unwrap();
    assert_eq!(request_json["sceneTag"], "MY_PUBLISH_ADD_EXPOSURE");
    assert_eq!(request_json["itemIdList"][0], "item-exposure-1");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_create_requires_confirm_and_forwards_real_order_payload() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_item(&xianyu_pool, "uid-growth", "item-exposure-1").await;

    let without_confirm = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "itemIds": ["item-exposure-1"],
                        "exposureNum": 100,
                        "expectedExposureNum": 100,
                        "picUrl": "https://img.example.test/item-exposure-1.jpg",
                        "title": "曝光商品"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(without_confirm.status(), StatusCode::OK);
    let payload = json_body(without_confirm).await;
    assert_eq!(
        payload["ret"][0],
        "FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认"
    );
    assert!(captured.lock().unwrap().is_empty());

    let mismatch = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "itemIds": ["item-exposure-1"],
                        "exposureNum": 100,
                        "expectedExposureNum": 50,
                        "picUrl": "https://img.example.test/item-exposure-1.jpg",
                        "title": "曝光商品",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::OK);
    let payload = json_body(mismatch).await;
    assert_eq!(
        payload["ret"][0],
        "FAIL_LOCAL_EXPECTED_EXPOSURE_MISMATCH::expectedExposureNum 必须等于 exposureNum"
    );
    assert!(captured.lock().unwrap().is_empty());

    let confirmed = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "itemIds": ["item-exposure-1"],
                        "exposureNum": 100,
                        "expectedExposureNum": 100,
                        "picUrl": "https://img.example.test/item-exposure-1.jpg",
                        "title": "曝光商品",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(confirmed.status(), StatusCode::OK);
    let payload = json_body(confirmed).await;
    assert_eq!(payload["data"]["orderId"], "exposure-order-1");

    let captured = captured.lock().unwrap().clone();
    let create = find_path(
        &captured,
        "/h5/mtop.taobao.idle.item.exposure.order.create/1.0/",
    );
    assert_eq!(create.data["itemIdList"], "item-exposure-1");
    assert_eq!(create.data["exposureNum"], 100);
    assert_eq!(
        create.data["picUrl"],
        "https://img.example.test/item-exposure-1.jpg"
    );
    assert_eq!(create.data["title"], "曝光商品");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_create_blocks_items_owned_by_other_accounts() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;
    seed_item(&xianyu_pool, "other-uid", "item-other").await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/create")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "itemIds": ["item-other"],
                        "exposureNum": 100,
                        "expectedExposureNum": 100,
                        "picUrl": "https://img.example.test/item-other.jpg",
                        "title": "其他账号商品",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::OK);
    assert!(captured.lock().unwrap().is_empty());

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_details_without_identifiers_returns_empty_without_upstream_call() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::get("/api/shop/exposure/details?uid=uid-growth&page=1&pageSize=20")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["detailList"], json!([]));
    assert_eq!(payload["data"]["skipped"], true);

    let captured = captured.lock().unwrap().clone();
    assert!(!captured.iter().any(|request| {
        request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.exposure.details/1.0/"
    }));

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_write_routes_require_confirm_before_calling_upstream() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let receive_without_confirm = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/shop/exposure/tasks/receive")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "taskId": "safe-award"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(receive_without_confirm.status(), StatusCode::OK);
    let receive_payload = json_body(receive_without_confirm).await;
    assert_eq!(
        receive_payload["ret"][0],
        "FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认"
    );

    let complete_without_confirm = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/shop/exposure/tasks/complete-view")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(complete_without_confirm.status(), StatusCode::OK);
    let complete_payload = json_body(complete_without_confirm).await;
    assert_eq!(complete_payload["data"]["success"], false);

    assert!(captured.lock().unwrap().is_empty());
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_run_route_requires_confirm_before_calling_upstream() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/tasks/run")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "taskId": "safe-accepted",
                        "taskType": "shop_visit_diagnosis"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(
        payload["ret"][0],
        "FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认"
    );
    assert!(captured.lock().unwrap().is_empty());

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_run_route_syncs_then_receives_for_verified_task_types() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/tasks/run")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "taskId": "safe-accepted",
                        "taskType": "shop_visit_diagnosis",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["success"], true);
    assert_eq!(payload["data"]["matchedTask"]["taskId"], "safe-accepted");
    assert_eq!(
        payload["data"]["matchedTask"]["taskType"],
        "shop_visit_diagnosis"
    );
    assert_eq!(payload["data"]["receive"]["data"]["received"], true);

    let captured = captured.lock().unwrap().clone();
    let sync_requests: Vec<_> = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.sync/1.0/")
        .collect();
    assert_eq!(sync_requests.len(), 1);
    assert_eq!(sync_requests[0].data["taskId"], "safe-accepted");
    assert_eq!(sync_requests[0].data["taskType"], "shop_visit_diagnosis");

    let receive_requests: Vec<_> = captured
        .iter()
        .filter(|request| {
            request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.receive.reward/1.0/"
        })
        .collect();
    assert_eq!(receive_requests.len(), 1);
    assert_eq!(receive_requests[0].data["taskId"], "safe-accepted");
    assert_eq!(receive_requests[0].data["benefitId"], "benefit-safe");

    let list_requests = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.list/1.0/")
        .count();
    assert_eq!(list_requests, 3);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_run_route_blocks_tasks_without_executable_identity_locally() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/tasks/run")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "taskType": "shop_visit_new_data_board",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(
        payload["ret"][0],
        "FAIL_LOCAL_MISSING_TASK_IDENTITY::缺少可执行任务身份，拒绝执行"
    );
    assert_eq!(payload["data"]["success"], false);
    assert!(captured.lock().unwrap().is_empty());

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_run_route_alias_paths_are_available() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    for path in [
        "/api/promotion/exposure/tasks/run",
        "/api/shop/exposure/tasks/run",
    ] {
        let response = app
            .clone()
            .oneshot(with_auth(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "uid": "uid-growth",
                            "taskId": "safe-accepted",
                            "taskType": "shop_visit_diagnosis",
                            "confirm": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let payload = json_body(response).await;
        assert_eq!(payload["data"]["success"], true, "{path}");
    }

    let captured = captured.lock().unwrap().clone();
    let sync_requests = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.sync/1.0/")
        .count();
    assert_eq!(sync_requests, 2);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn complete_view_only_syncs_and_receives_safe_view_task_types() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/shop/exposure/tasks/complete-view")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["success"], true);
    assert_eq!(payload["data"]["syncedTaskIds"], json!(["safe-accepted"]));
    assert_eq!(
        payload["data"]["receivedRewards"][0]["taskId"],
        "safe-accepted"
    );
    assert_eq!(
        payload["data"]["receivedRewards"][1]["taskId"],
        "safe-award"
    );

    let captured = captured.lock().unwrap().clone();
    let sync_requests: Vec<_> = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.sync/1.0/")
        .collect();
    assert_eq!(sync_requests.len(), 1);
    assert_eq!(sync_requests[0].data["taskId"], "safe-accepted");
    assert_eq!(sync_requests[0].data["taskType"], "shop_visit_diagnosis");

    let receive_requests: Vec<_> = captured
        .iter()
        .filter(|request| {
            request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.receive.reward/1.0/"
        })
        .collect();
    assert_eq!(receive_requests.len(), 2);
    assert_eq!(receive_requests[0].data["taskId"], "safe-accepted");
    assert_eq!(receive_requests[0].data["benefitId"], "benefit-safe");
    assert_eq!(receive_requests[1].data["taskId"], "safe-award");
    assert_eq!(receive_requests[1].data["benefitId"], "benefit-existing");

    let unsafe_types = [
        "shop_subscribe_msg",
        "shop_open_groupon",
        "shop_improvePack_open",
        "shop_order_one_step",
        "shop_main_pubilsh",
    ];
    for request in sync_requests.iter().chain(receive_requests.iter()) {
        let task_type = request.data["taskType"].as_str().unwrap_or_default();
        assert!(
            !unsafe_types.contains(&task_type),
            "unsafe task type was executed: {task_type}"
        );
    }

    let list_requests = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.list/1.0/")
        .count();
    assert_eq!(list_requests, 3);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn complete_view_syncs_structurally_safe_exposure_view_tasks_without_fixed_type() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState {
        dynamic_exposure_shapes: true,
        ..UpstreamState::default()
    };
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/shop/exposure/tasks/complete-view")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["success"], true);
    assert_eq!(payload["data"]["syncedTaskIds"], json!(["dynamic-view"]));
    assert_eq!(
        payload["data"]["receivedRewards"][0]["taskId"],
        "dynamic-view"
    );
    assert!(payload["data"]["taskCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["taskId"] == "dynamic-view" && task["autoSafe"] == true));
    assert!(payload["data"]["skippedTasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["taskId"] == "dynamic-publish" && task["reason"] == "manual_risk_task"));

    let captured = captured.lock().unwrap().clone();
    let sync_requests: Vec<_> = captured
        .iter()
        .filter(|request| request.path == "/h5/mtop.alibaba.idle.shop.incentive.task.sync/1.0/")
        .collect();
    assert_eq!(sync_requests.len(), 1);
    assert_eq!(sync_requests[0].data["taskId"], "dynamic-view");
    assert_eq!(
        sync_requests[0].data["taskType"],
        "shop_visit_new_data_board"
    );

    assert!(!captured.iter().any(|request| {
        request.data["taskId"] == "dynamic-publish"
            || request.data["taskType"] == "shop_publish_new_item"
    }));

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn confirmed_receive_reward_forwards_payload_unchanged_except_confirm() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/shop/exposure/tasks/receive")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "taskId": "safe-award",
                        "taskType": "shop_visit_xy_coin",
                        "benefitId": "benefit-direct",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["received"], true);

    let captured = captured.lock().unwrap().clone();
    let receive_request = find_path(
        &captured,
        "/h5/mtop.alibaba.idle.shop.incentive.task.receive.reward/1.0/",
    );
    assert_eq!(receive_request.data["taskId"], "safe-award");
    assert_eq!(receive_request.data["taskType"], "shop_visit_xy_coin");
    assert_eq!(receive_request.data["benefitId"], "benefit-direct");
    assert!(receive_request.data.get("confirm").is_none());

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn growth_super_polish_trigger_reuses_dedicated_module_and_requires_confirm() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let without_confirm = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/super-polish/trigger")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "uid": "uid-growth" }).to_string()))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(without_confirm.status(), StatusCode::OK);
    let payload = json_body(without_confirm).await;
    assert_eq!(
        payload["ret"][0],
        "FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认"
    );
    assert!(captured.lock().unwrap().is_empty());

    let confirmed = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/super-polish/trigger")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(confirmed.status(), StatusCode::OK);
    let payload = json_body(confirmed).await;
    assert_eq!(payload["data"]["triggered"], true);
    assert_eq!(payload["data"]["transportMeta"]["preferred"], "h5");
    assert_eq!(payload["data"]["transportMeta"]["chosen"], "h5");
    assert_eq!(payload["data"]["transportMeta"]["fallbackUsed"], false);

    let captured = captured.lock().unwrap().clone();
    let trigger = find_path(&captured, "/h5/mtop.taobao.idle.super.exposure/1.0/");
    assert!(trigger.data.get("fishAdCode").is_some());
    assert!(trigger.data.get("gps").is_some());

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn growth_super_polish_account_falls_back_to_h5_when_app_transport_is_not_success() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState {
        force_app_super_polish_account_fail: true,
        ..UpstreamState::default()
    };
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
            Request::get("/api/growth/super-polish/account?uid=uid-growth")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["usableBalanceOfYuan"], "12.50");
    assert_eq!(payload["data"]["transportMeta"]["preferred"], "app");
    assert_eq!(payload["data"]["transportMeta"]["chosen"], "h5");
    assert_eq!(payload["data"]["transportMeta"]["fallbackUsed"], true);

    let captured = captured.lock().unwrap().clone();
    find_path(
        &captured,
        "/app/mtop.taobao.idle.ad.follow.super.polish.account.query/1.0/",
    );
    find_path(
        &captured,
        "/h5/mtop.taobao.idle.ad.follow.super.polish.account.query/1.0/",
    );

    set_sign_license_context(original_license);
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn community_app_request_uses_h5_when_signing_context_is_absent() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    configure_app_transport_env(&base_url);
    let original_license = get_sign_license_context();
    set_sign_license_context(None);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let response = app
        .oneshot(with_auth(
            Request::get("/api/growth/super-polish/account?uid=uid-growth")
                .body(Body::empty())
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["usableBalanceOfYuan"], "12.50");

    let captured = captured.lock().unwrap().clone();
    find_path(
        &captured,
        "/h5/mtop.taobao.idle.ad.follow.super.polish.account.query/1.0/",
    );
    assert!(!captured.iter().any(|request| {
        request.path == "/app/mtop.taobao.idle.ad.follow.super.polish.account.query/1.0/"
    }));

    set_sign_license_context(original_license);
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn super_polish_namespace_uses_fishad_profile_and_request_json_wrapping() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    for request in [
        Request::get("/api/super-polish/home?uid=uid-growth")
            .body(Body::empty())
            .unwrap(),
        Request::get("/api/super-polish/account/login-info?uid=uid-growth")
            .body(Body::empty())
            .unwrap(),
        Request::get("/api/super-polish/campaigns?uid=uid-growth&pageNum=1&pageSize=10")
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = app.clone().oneshot(with_auth(request)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let search_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/super-polish/campaigns/items/search")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "pageNum": 1,
                        "pageSize": 10,
                        "tab": "all",
                        "needTabs": false,
                        "searchKey": ""
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(search_response.status(), StatusCode::OK);

    let report_response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/super-polish/reports/group")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "startDate": "20260612",
                        "endDate": "20260612",
                        "includeEndDate": true,
                        "adGroupId": "100990381",
                        "allPeriod": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(report_response.status(), StatusCode::OK);

    let captured = captured.lock().unwrap().clone();

    let home = find_path(&captured, "/h5/mtop.taobao.idle.ad.home/1.0/");
    assert_eq!(
        home.headers.get("origin").map(String::as_str),
        Some("https://dayu.goofish.com")
    );
    assert_eq!(
        home.headers.get("referer").map(String::as_str),
        Some("https://dayu.goofish.com/")
    );
    assert_eq!(
        home.headers.get("idle_site_biz_code").map(String::as_str),
        Some("FISHAD")
    );

    find_path(&captured, "/h5/mtop.taobao.idle.ad.get.login.info.v2/1.0/");

    let campaigns = find_path(&captured, "/h5/mtop.taobao.idle.ad.campaign.search/1.0/");
    assert!(campaigns.data["requestJson"]
        .as_str()
        .unwrap_or_default()
        .contains("\"pageNum\":\"1\""));
    assert!(campaigns.data["requestJson"]
        .as_str()
        .unwrap_or_default()
        .contains("\"pageSize\":\"10\""));

    let search = find_path(&captured, "/h5/mtop.taobao.idle.ad.item.searchV2/1.0/");
    assert!(search.data["requestJson"]
        .as_str()
        .unwrap_or_default()
        .contains("\"tab\":\"all\""));
    assert!(search.data["requestJson"]
        .as_str()
        .unwrap_or_default()
        .contains("\"needTabs\":false"));

    let group_report = find_path(&captured, "/h5/mtop.taobao.idle.ad.group.data.query/1.0/");
    assert!(group_report.data["requestJson"]
        .as_str()
        .unwrap_or_default()
        .contains("\"adGroupId\":\"100990381\""));

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn growth_super_polish_trigger_falls_back_to_app_when_h5_transport_is_not_success() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState {
        force_h5_super_polish_trigger_fail: true,
        ..UpstreamState::default()
    };
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
            Request::post("/api/growth/super-polish/trigger")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["triggered"], true);
    assert_eq!(payload["data"]["transportMeta"]["preferred"], "h5");
    assert_eq!(payload["data"]["transportMeta"]["chosen"], "app");
    assert_eq!(payload["data"]["transportMeta"]["fallbackUsed"], true);

    let captured = captured.lock().unwrap().clone();
    find_path(&captured, "/h5/mtop.taobao.idle.super.exposure/1.0/");
    find_path(&captured, "/app/mtop.taobao.idle.super.exposure/1.0/");

    set_sign_license_context(original_license);
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn exposure_sync_route_forwards_single_task_payload_after_confirm() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let captured = upstream.requests.clone();
    let (base_url, shutdown_tx) = spawn_upstream(upstream).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let without_confirm = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/tasks/sync")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "taskId": "safe-accepted",
                        "taskType": "shop_visit_diagnosis"
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(without_confirm.status(), StatusCode::OK);
    let without_confirm_payload = json_body(without_confirm).await;
    assert_eq!(
        without_confirm_payload["ret"][0],
        "FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认"
    );

    let response = app
        .clone()
        .oneshot(with_auth(
            Request::post("/api/growth/exposure/tasks/sync")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-growth",
                        "taskId": "safe-accepted",
                        "taskType": "shop_visit_diagnosis",
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["data"]["synced"], true);

    let captured = captured.lock().unwrap().clone();
    let sync_request = find_path(
        &captured,
        "/h5/mtop.alibaba.idle.shop.incentive.task.sync/1.0/",
    );
    assert_eq!(sync_request.data["taskId"], "safe-accepted");
    assert_eq!(sync_request.data["taskType"], "shop_visit_diagnosis");
    assert!(sync_request.data.get("confirm").is_none());

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
    record_request(&state, path.clone(), query, &headers, &body);

    match path.as_str() {
        "/h5/mtop.alibaba.idle.shop.incentive.task.list/1.0/" => {
            let mut calls = state.task_list_calls.lock().unwrap();
            *calls += 1;
            let payload = if state.dynamic_exposure_shapes {
                dynamic_task_list_response(*calls)
            } else if state.real_exposure_shapes {
                captured_shape_task_list_response(*calls)
            } else {
                task_list_response(*calls)
            };
            axum::Json(payload).into_response()
        }
        "/h5/mtop.alibaba.idle.shop.incentive.task.sync/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "synced": true }
        }))
        .into_response(),
        "/h5/mtop.alibaba.idle.shop.incentive.task.receive.reward/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "received": true }
        }))
        .into_response(),
        "/h5/mtop.alibaba.idle.shop.incentive.task.exposure.history/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "items": [] }
        }))
        .into_response(),
        "/h5/mtop.alibaba.idle.shop.incentive.task.exposure.details/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "itemId": "item-1" }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.item.exposure.coin.home/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "exposureCoinNum": 120,
                "exposeableItemDOList": [
                    { "itemId": "item-exposure-1", "title": "曝光商品" }
                ],
                "exposureOrderList": []
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.item.exposure.order.amount.select/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "hasQuantity": true,
                "userAccount": 120,
                "exposureAmountDOList": [
                    { "exposureNum": 50, "title": "推荐补量" },
                    { "exposureNum": 100, "title": "集中投放" }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.item.exposure.order.create/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "orderId": "exposure-order-1",
                "success": true
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.ad.item.activity.rendering/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "itemActivityInstanceVOList": []
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.pro.workbench.info.get/3.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "workbench": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.play.sign.in.home/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "signed": false, "coinAmount": 50 }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.play.sign.in/1.0/" => axum::Json(json!({
            "ret": ["FAIL_BIZ_PLAY_SIGNED_IN::当日已签到~"],
            "data": { "success": false }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.activity.user.info/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "myIdleCoin": 123, "todaySignIn": false, "alreadySignInDays": 6 }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.user.detail.list/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "nextPage": false,
                "userScoreDetailDOList": [
                    { "describe": "签到奖励", "score": 50, "time": "2026-06-10 10:00:00" }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.virtual.card.list/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "data": [
                    { "title": "推广抵扣券", "status": "AVAILABLE", "amount": 1 }
                ],
                "hasNextPage": false,
                "success": true
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.task.getolivertaskdetail/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "taskDTO": {
                    "deliveryId": 94398,
                    "sceneId": 496
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.wlj.task.detail/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "deliveryId": 94398,
                "sceneId": 496,
                "taskCode": "wlj-task"
            }
        }))
        .into_response(),
        "/h5/mtop.idle.challenge.idleCoinSign.info/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "failedFlag": false }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.subscribe.get/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "subscribe": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.task.queryolivertasklist/2.0/" => {
            let mut calls = state.coin_task_list_calls.lock().unwrap();
            *calls += 1;
            let progressed = state
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request.path == "/h5/mtop.taobao.idle.play.ad.event.push/1.0/");
            let payload = if state.real_coin_view_shapes {
                coin_task_list_response(*calls, progressed)
            } else {
                json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": { "taskList": [] }
                })
            };
            axum::Json(payload).into_response()
        }
        "/h5/mtop.taobao.idle.play.ad.event.push/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "success": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.task.getolivertaskbenefit/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "received": true }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.promotion.view/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "success": true,
                "promotionPriceMap": {
                    "50": { "nowPrice": 1500, "maxPageView": 50, "days": 1 },
                    "100": { "nowPrice": 4500, "maxPageView": 100, "days": 1 },
                    "300": { "nowPrice": 13500, "maxPageView": 300, "days": 1 }
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.promotion.page/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "items": [
                    { "id": "promo-1", "aimedPromoteNum": 50, "promotionStatus": 2 }
                ],
                "serverTime": 1779926607506_i64
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.promotion.detail/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "promotionDetailVO": {
                    "id": "promo-1",
                    "payCoin": 1500,
                    "realCoin": 0,
                    "returnCoin": 0,
                    "promotionStatus": 2
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.coin.promotion.create/2.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "activityId": 827559279_i64,
                "serverTime": "2026-05-28 08:03:27"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.ad.home/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "campaignInfo": { "allCampaignCnt": 1 },
                "bannerList": [],
                "allAccountBalanceDetails": []
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.ad.get.login.info.v2/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "displayNick": "Growth Seller",
                "operatorDisplayNick": "Growth Seller"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.ad.campaign.search/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "nextPage": false,
                "result": []
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.ad.item.searchV2/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "nextPage": false,
                "result": { "itemList": [] }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.ad.group.data.query/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "data": { "clickPv": 0, "cost": 0 },
                "dataDetailVOList": []
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.ad.follow.super.polish.account.query/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::接口调用成功"],
            "data": {
                "account": true,
                "usableBalanceOfYuan": "12.50",
                "status": "正常"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.super.exposure/1.0/" => {
            let payload = if state.force_h5_super_polish_trigger_fail {
                json!({
                    "ret": ["FAIL_SYS_H5_RETRY::请切换通道后重试"],
                    "data": { "triggered": false }
                })
            } else {
                json!({
                    "ret": ["SUCCESS::调用成功"],
                    "data": { "triggered": true }
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

async fn app_handler(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let body = request.into_body().collect().await.unwrap().to_bytes();
    record_request(&state, path.clone(), HashMap::new(), &headers, &body);

    match path.as_str() {
        "/app/mtop.taobao.idle.ad.follow.super.polish.account.query/1.0/" => {
            let payload = if state.force_app_super_polish_account_fail {
                json!({
                    "ret": ["FAIL_SYS_APP_RETRY::app transport temporary failed"],
                    "data": { "account": false }
                })
            } else {
                json!({
                    "ret": ["SUCCESS::接口调用成功"],
                    "data": {
                        "account": true,
                        "usableBalanceOfYuan": "12.50",
                        "status": "正常"
                    }
                })
            };
            axum::Json(payload).into_response()
        }
        "/app/mtop.taobao.idle.super.exposure/1.0/" => axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "triggered": true }
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

async fn resolve_bindings_handler(body: Body) -> impl IntoResponse {
    let bytes = body.collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}));
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

fn captured_shape_task_list_response(call: usize) -> Value {
    let status = if call == 1 { "ACCEPTED" } else { "AWARD" };
    json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "activityId": "100769",
            "incentiveTask": {
                "viewTaskList": [
                    {
                        "taskInstanceId": "1007694526",
                        "taskType": "shop_visit_xy_coin",
                        "taskStatus": status,
                        "benefitVO": {
                            "activityId": "100769",
                            "taskInstanceId": "1007694526",
                            "rewardList": [
                                {
                                    "denomination": 20,
                                    "rewardType": "FLOW_AMOUNT"
                                }
                            ]
                        }
                    },
                    {
                        "taskInstanceId": "1007699000",
                        "taskType": "shop_open_groupon",
                        "taskStatus": "ACCEPTED",
                        "benefitVO": {
                            "activityId": "100769",
                            "taskInstanceId": "1007699000",
                            "rewardList": [
                                {
                                    "denomination": 100,
                                    "rewardType": "FLOW_AMOUNT"
                                }
                            ]
                        }
                    }
                ]
            }
        }
    })
}

fn task_list_response(call: usize) -> Value {
    let safe_status = if call == 1 { "ACCEPTED" } else { "AWARD" };
    json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "taskList": [
                {
                    "taskId": "safe-accepted",
                    "taskType": "shop_visit_diagnosis",
                    "status": safe_status,
                    "benefitId": "benefit-safe"
                },
                {
                    "taskId": "safe-award",
                    "taskType": "shop_visit_xy_coin",
                    "status": "AWARD",
                    "benefitId": "benefit-existing"
                },
                {
                    "taskId": "unsafe-subscribe",
                    "taskType": "shop_subscribe_msg",
                    "status": "ACCEPTED",
                    "benefitId": "benefit-unsafe-subscribe"
                },
                {
                    "taskId": "unsafe-groupon",
                    "taskType": "shop_open_groupon",
                    "status": "ACCEPTED",
                    "benefitId": "benefit-unsafe-groupon"
                },
                {
                    "taskId": "unsafe-pack-open",
                    "taskType": "shop_improvePack_open",
                    "status": "ACCEPTED",
                    "benefitId": "benefit-unsafe-pack"
                },
                {
                    "taskId": "unsafe-order",
                    "taskType": "shop_order_one_step",
                    "status": "ACCEPTED",
                    "benefitId": "benefit-unsafe-order"
                },
                {
                    "taskId": "unsafe-publish",
                    "taskType": "shop_main_pubilsh",
                    "status": "ACCEPTED",
                    "benefitId": "benefit-unsafe-publish"
                }
            ]
        }
    })
}

fn dynamic_task_list_response(call: usize) -> Value {
    let status = if call == 1 { "ACCEPTED" } else { "AWARD" };
    json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "taskList": [
                {
                    "taskId": "dynamic-view",
                    "taskType": "shop_visit_new_data_board",
                    "status": status,
                    "benefitId": "benefit-dynamic-view",
                    "materialVO": {
                        "title": "查看新的小铺数据看板",
                        "desc": "浏览后领取曝光奖励"
                    }
                },
                {
                    "taskId": "dynamic-publish",
                    "taskType": "shop_publish_new_item",
                    "status": "ACCEPTED",
                    "benefitId": "benefit-dynamic-publish",
                    "materialVO": {
                        "title": "发布一个宝贝",
                        "desc": "发布商品后领取曝光奖励"
                    }
                }
            ]
        }
    })
}

fn coin_task_list_response(call: usize, progressed: bool) -> Value {
    let (status, times, need_times) = if progressed {
        match call {
            1 => ("ACCEPTED", 0, 1),
            2 => ("AWARD", 1, 0),
            _ => ("COMPLETE", 1, 0),
        }
    } else {
        ("ACCEPTED", 0, 1)
    };
    json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "taskList": [
                {
                    "deliveryId": "79493",
                    "sceneId": 496,
                    "idleTaskStatus": status,
                    "materialDTO": {
                        "title": "去浏览全新好物",
                        "actionType": "VISIT",
                        "action": "https://h5.m.goofish.com/wow/moyu/moyu-project/alimama-ads/pages/home?eventType=award&orderCode=mmOrderView02&adCode=mmOrder_view01&isView=true&source=xybrworder",
                        "desc": "80"
                    },
                    "progressDTO": {
                        "times": times,
                        "needTimes": need_times
                    }
                },
                {
                    "deliveryId": "74620",
                    "sceneId": 496,
                    "idleTaskStatus": status,
                    "groupId": 39264,
                    "implId": "other_713_2327016_74620_0",
                    "fromToken": "from-token-74620",
                    "materialDTO": {
                        "title": "搜一搜推荐商品",
                        "actionType": "VISIT",
                        "action": "https://ku.m.taobao.com/xianyu/search.html?titleVisible=false&adPositionCode=xybAlimmss&implId=xyb001&channelSrp=xybrwNew",
                        "desc": "100",
                        "extra": {
                            "oliverActivityId": "168",
                            "oliverBenefitCode": "10617930650327691765",
                            "oliverType": 0
                        }
                    },
                    "taskBenefitDTO": {
                        "oliverActivityId": 168,
                        "oliverBenefitCode": "10617930650327691765",
                        "oliverType": 0
                    },
                    "progressDTO": {
                        "times": times,
                        "needTimes": need_times
                    }
                },
                {
                    "deliveryId": "79496",
                    "sceneId": 496,
                    "idleTaskStatus": "ACCEPTED",
                    "materialDTO": {
                        "title": "搜一搜推荐商品下单",
                        "actionType": "VISIT",
                        "action": "https://ku.m.taobao.com/xianyu/search.html?titleVisible=false&adPositionCode=mmOrder_buy01",
                        "desc": "100"
                    },
                    "progressDTO": {
                        "times": 0,
                        "needTimes": 1
                    }
                },
                {
                    "deliveryId": "88299",
                    "sceneId": 496,
                    "idleTaskStatus": "ACCEPTED",
                    "materialDTO": {
                        "title": "浏览鱼小铺工作台",
                        "actionType": "clickFinish",
                        "action": "https://h5.m.goofish.com/wow/moyu/moyu-project/fish-pro-workbench/pages/Workbench?from=xianyubi",
                        "desc": "80"
                    },
                    "progressDTO": {
                        "times": 0,
                        "needTimes": 1
                    }
                }
            ]
        }
    })
}

fn record_request(
    state: &UpstreamState,
    path: String,
    query: HashMap<String, String>,
    headers: &HeaderMap,
    raw_body: &[u8],
) {
    state.requests.lock().unwrap().push(CapturedRequest {
        path,
        data: extract_data_json(raw_body),
        query,
        headers: capture_headers(headers),
    });
}

fn capture_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|parsed| (name.as_str().to_ascii_lowercase(), parsed.to_string()))
        })
        .collect()
}

fn extract_data_json(raw_body: &[u8]) -> Value {
    let decoded = percent_decode(raw_body);
    let data = decoded.strip_prefix("data=").unwrap_or(decoded.as_str());
    serde_json::from_str(data).unwrap_or(Value::Null)
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
    env::remove_var("XIANYU_WX_GATEWAY");
    env::remove_var("XIANYU_APP_GATEWAY");
    env::remove_var("XIANYU_APP_MTOP_GATEWAY");
    env::remove_var("XIANYU_STREAM_UPLOAD_URL");
    env::remove_var("XIANYU_UPLOAD_TAOBAO_URL");
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
    .bind("uid-growth")
    .bind("Growth Seller")
    .bind("seed-growth-token_1710000000000")
    .bind("sid-growth")
    .bind(
        json!({
            "_m_h5_tk": "seed-growth-token_1710000000000",
            "_m_h5_tk_enc": "seed-growth-token-enc",
            "cookie2": "cookie-two",
            "t": "token-t"
        })
        .to_string(),
    )
    .bind(json!({}).to_string())
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

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn find_path<'a>(captured: &'a [CapturedRequest], path: &str) -> &'a CapturedRequest {
    captured
        .iter()
        .find(|request| request.path == path)
        .unwrap_or_else(|| panic!("missing captured request path: {path}"))
}
