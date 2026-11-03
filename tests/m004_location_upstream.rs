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

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    body: String,
    cookie: String,
}

#[tokio::test]
async fn location_routes_prefer_real_upstream_payloads() {
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

    let divisions = request_json(app.clone(), "/api/location/divisions?uid=uid-upstream").await;
    assert_eq!(divisions["ret"][0], "SUCCESS::调用成功");
    assert_eq!(divisions["divisionArea"][0]["na"], "湖南省");
    assert_eq!(divisions["divisionArea"][1]["na"], "浙江省");

    let children = request_json(
        app.clone(),
        "/api/location/division_children?uid=uid-upstream&parent_id=430000",
    )
    .await;
    assert_eq!(children["ret"][0], "SUCCESS::调用成功");
    assert_eq!(children["items"][0]["na"], "株洲市");

    let by_gps = request_json(
        app.clone(),
        "/api/location/by_gps?uid=uid-upstream&lat=27.867187&lng=113.164441",
    )
    .await;
    assert_eq!(by_gps["ret"][0], "SUCCESS::调用成功");
    assert_eq!(by_gps["prov"], "湖南省");
    assert_eq!(by_gps["city"], "株洲市");
    assert_eq!(by_gps["area"], "天元区");
    assert_eq!(by_gps["divisionId"], "430211");

    let poi = request_json(
        app.clone(),
        "/api/location/poi_search?uid=uid-upstream&keyword=%E6%B9%96%E5%8D%97%E5%B7%A5%E4%B8%9A%E5%A4%A7%E5%AD%A6",
    )
    .await;
    assert_eq!(poi["ret"][0], "SUCCESS::调用成功");
    assert_eq!(poi["items"][0]["poi"], "湖南工业大学");
    assert_eq!(poi["items"][0]["city"], "株洲市");
    assert_eq!(poi["items"][0]["divisionId"], "430211");

    let captured = upstream.requests.lock().unwrap().clone();
    assert!(captured.iter().any(|request| request.path
        == "/h5/mtop.taobao.idle.division.all.get/3.0/"
        && !request.body.contains("divisionId")));
    assert!(captured.iter().any(|request| request.path
        == "/h5/mtop.taobao.idle.division.all.get/3.0/"
        && request.body.contains("430000")));
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idle.division.by.gps/1.0/"));
    assert!(captured.iter().any(|request| request.path
        == "/h5/mtop.taobao.idle.local.poi.search/1.0/"
        && request.body.contains("keywords")));
    assert!(captured
        .iter()
        .all(|request| request.cookie.contains("uid-upstream")));

    let _ = shutdown_tx.send(());
    clear_env();
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
        body: body.clone(),
        cookie,
    });

    match path.as_str() {
        "/h5/mtop.taobao.idle.division.all.get/3.0/" => {
            let division_area = if body.contains("430000") {
                vec![
                    json!({ "id": 430200, "na": "株洲市", "level": "city", "hasChild": true }),
                    json!({ "id": 430700, "na": "常德市", "level": "city", "hasChild": true }),
                ]
            } else {
                vec![
                    json!({ "id": 430000, "na": "湖南省", "level": "prov", "hasChild": true }),
                    json!({ "id": 330000, "na": "浙江省", "level": "prov", "hasChild": true }),
                ]
            };
            Json(json!({
                "ret": ["SUCCESS::调用成功"],
                "data": {
                    "divisionArea": division_area
                }
            }))
            .into_response()
        }
        "/h5/mtop.taobao.idle.division.by.gps/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "prov": "湖南省",
                "city": "株洲市",
                "area": "天元区",
                "divisionId": "430211"
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idle.local.poi.search/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "searchPois": [
                    {
                        "poi": "湖南工业大学",
                        "address": "湖南省株洲市天元区泰山西路88号",
                        "prov": "湖南省",
                        "city": "株洲市",
                        "area": "天元区",
                        "divisionId": "430211",
                        "latitude": "27.829600",
                        "longitude": "113.132900",
                        "poiId": "poi-hn-gydx"
                    }
                ]
            }
        }))
        .into_response(),
        _ => (StatusCode::NOT_FOUND, "not found").into_response(),
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
            extra,
            security_score,
            security_score_updated_at,
            restriction_end,
            coin_balance,
            violation_count,
            fish_power,
            seller_30d_sold
        )
        VALUES (?, ?, ?, ?, '', ?, '[]', '', 'active', 0, 0, 0, 0, 0, 0, '', '', '', ?, -1, 0, '', 0, 0, 0, '')
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
    .bind(
        json!({
            "location": {
                "fishAdCode": "430202",
                "gps": "27.867187,113.164441",
                "latitude": "27.867187",
                "longitude": "113.164441",
                "city": "株洲市",
                "province": "湖南省",
                "area": "荷塘区"
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

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::set_var("API_TOKEN", "test-api-token");
}

fn clear_env() {
    env::remove_var("APP_DATA_DIR");
    env::remove_var("JWT_SECRET");
    env::remove_var("ADMIN_USERNAME");
    env::remove_var("ADMIN_PASSWORD");
    env::remove_var("API_TOKEN");
    env::remove_var("XIANYU_H5_GATEWAY");
}
