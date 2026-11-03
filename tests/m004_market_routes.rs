use std::{env, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app, auth::encode_access_token, config::AppConfig, state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn market_routes_cover_publish_items_trade_and_upload_baselines() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let admin_auth = format!(
        "Bearer {}",
        encode_access_token(&config.auth, "admin", "admin", false).unwrap()
    );
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;

    seed_account(&accounts_pool, "admin").await;
    seed_order(&xianyu_pool).await;
    seed_item(&xianyu_pool).await;

    let preget_response = app
        .clone()
        .oneshot(
            Request::post("/api/publish/preget?uid=uid-1")
                .header("authorization", &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preget_response.status(), StatusCode::OK);
    let preget_payload = json_body(preget_response).await;
    assert_eq!(preget_payload["ret"][0], "SUCCESS::调用成功");

    let item_id = "item-1".to_string();

    let items_response = app
        .clone()
        .oneshot(
            Request::post("/api/items")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "page": 1,
                        "page_size": 50,
                        "tab": "ON_SALE"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(items_response.status(), StatusCode::OK);
    let items_payload = json_body(items_response).await;
    assert_eq!(
        items_payload["data"]["itemList"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(items_payload["data"]["itemList"][0]["itemId"], item_id);

    let comment_publish_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/comment/publish")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "item_id": item_id,
                        "content": "灯很不错"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(comment_publish_response.status(), StatusCode::OK);
    let comment_publish_payload = json_body(comment_publish_response).await;
    let comment_id = comment_publish_payload["data"]["commentId"]
        .as_str()
        .unwrap()
        .to_string();

    let comment_list_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/comment/list")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "item_id": item_id
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(comment_list_response.status(), StatusCode::OK);
    let comment_list_payload = json_body(comment_list_response).await;
    assert_eq!(comment_list_payload["ret"][0], "SUCCESS::调用成功");

    let ratio_response = app
        .clone()
        .oneshot(
            Request::get("/api/coin/deduction/ratio?uid=uid-1&price=88")
                .header("authorization", &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ratio_response.status(), StatusCode::OK);
    let ratio_payload = json_body(ratio_response).await;
    assert!(ratio_payload["ret"].as_array().is_some());

    let downshelf_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/downshelf")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "item_id": item_id
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(downshelf_response.status(), StatusCode::OK);

    let sold_out_items_response = app
        .clone()
        .oneshot(
            Request::post("/api/items")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "page": 1,
                        "page_size": 50,
                        "tab": "SOLD_OUT"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sold_out_items_response.status(), StatusCode::OK);
    let sold_out_items_payload = json_body(sold_out_items_response).await;
    assert_eq!(
        sold_out_items_payload["data"]["itemList"][0]["itemId"],
        item_id
    );

    let sold_response = app
        .clone()
        .oneshot(
            Request::get("/api/trade/sold?uid=uid-1&page=1&status=all")
                .header("authorization", &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sold_response.status(), StatusCode::OK);
    let sold_payload = json_body(sold_response).await;
    assert_eq!(
        sold_payload["data"]["orderList"].as_array().map(Vec::len),
        Some(1)
    );

    let order_adjust_response = app
        .clone()
        .oneshot(
            Request::post("/api/order/adjust_price")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "biz_order_id": "order-1",
                        "modify_fee": "66.00"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(order_adjust_response.status(), StatusCode::OK);

    let order_close_response = app
        .clone()
        .oneshot(
            Request::post("/api/order/close")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "biz_order_id": "order-1",
                        "close_reason": "不想卖了"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(order_close_response.status(), StatusCode::OK);

    let upload_response = app
        .clone()
        .oneshot(multipart_request(
            "/api/item/upload_image",
            "uid-1",
            "lamp.jpg",
            "image/jpeg",
            b"binary-image",
            &admin_auth,
        ))
        .await
        .unwrap();
    assert_eq!(upload_response.status(), StatusCode::BAD_REQUEST);

    let delete_comment_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/comment/delete")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "item_id": item_id,
                        "comment_id": comment_id
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_comment_response.status(), StatusCode::OK);

    let delete_item_response = app
        .clone()
        .oneshot(
            Request::post("/api/item/delete")
                .header("authorization", &admin_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "item_id": item_id
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_item_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn ordinary_user_cannot_call_raw_mtop() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let user_auth = format!(
        "Bearer {}",
        encode_access_token(&config.auth, "user-a", "user", false).unwrap()
    );
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool, "user-a").await;

    let response = app
        .oneshot(
            Request::post("/api/mtop")
                .header("authorization", &user_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "api_name": "mtop.taobao.idle.anything",
                        "api_version": "1.0",
                        "data": {"arbitrary": true}
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn ordinary_user_cannot_operate_order_not_in_local_cache() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let user_auth = format!(
        "Bearer {}",
        encode_access_token(&config.auth, "user-a", "user", false).unwrap()
    );
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;

    seed_account(&accounts_pool, "user-a").await;
    seed_order(&xianyu_pool).await;

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/order/close")
                .header("authorization", &user_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "biz_order_id": "order-not-owned",
                        "close_reason": "不想卖了"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let close_reason_response = app
        .oneshot(
            Request::get("/api/order/close_reason?uid=uid-1&biz_order_id=order-not-owned")
                .header("authorization", &user_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(close_reason_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ordinary_user_cannot_operate_item_not_in_local_cache() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let user_auth = format!(
        "Bearer {}",
        encode_access_token(&config.auth, "user-a", "user", false).unwrap()
    );
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;

    seed_account(&accounts_pool, "user-a").await;
    seed_item(&xianyu_pool).await;

    for path in [
        "/api/item/edit_detail",
        "/api/item/groupon_status",
        "/api/item/groupon_guide",
        "/api/item/properties",
        "/api/item/reduce_price_feeds",
        "/api/item/comment/list",
        "/api/item/comment/capture",
        "/api/item/comment/publish",
        "/api/item/comment/delete",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("authorization", &user_auth)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "uid": "uid-1",
                            "item_id": "item-not-owned",
                            "content": "不应该发送",
                            "comment_id": "comment-not-owned"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }

    let coin_promotion_response = app
        .clone()
        .oneshot(
            Request::post("/api/coin/promotion/create")
                .header("authorization", &user_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "itemId": "item-not-owned",
                        "costCoinNum": 1500,
                        "expectedCostCoinNum": 1500,
                        "confirm": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(coin_promotion_response.status(), StatusCode::NOT_FOUND);

    let response = app
        .oneshot(
            Request::post("/api/item/delete")
                .header("authorization", &user_auth)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-1",
                        "item_id": "item-not-owned"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

fn multipart_request(
    path: &str,
    uid: &str,
    filename: &str,
    content_type: &str,
    data: &[u8],
    auth_header: &str,
) -> Request<Body> {
    let boundary = "BOUNDARY123";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"uid\"\r\n\r\n");
    body.extend_from_slice(uid.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    Request::post(path)
        .header("authorization", auth_header)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::remove_var("API_TOKEN");
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

async fn seed_account(pool: &SqlitePool, owner_username: &str) {
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
        VALUES (?, ?, ?, '', '', ?, '', 'active', 0, 0, 0, 0, 0, 0, '', '', '{}', 95, 0, '', 0, 0, 0, 0)
        "#,
    )
    .bind("uid-1")
    .bind(owner_username)
    .bind("Seller One")
    .bind(r#"{"_m_h5_tk":"seed-route-token_1700000000000","_m_h5_tk_enc":"seed-route-enc"}"#)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_order(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO account_orders (
            tid,
            uid,
            title,
            price,
            pic_url,
            status,
            status_text,
            buyer_id,
            buyer_nick,
            buyer_avatar,
            gmt_create,
            created_at,
            updated_at
        )
        VALUES (?, ?, ?, ?, '', 'WAIT_SELLER_SEND_GOODS', '待发货', 'buyer-1', 'Buyer One', '', '2026-04-17 10:00:00', 0, 0)
        "#,
    )
    .bind("order-1")
    .bind("uid-1")
    .bind("Desk Lamp Order")
    .bind("88.00")
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_item(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO account_items (
            uid, item_id, title, description, price, status, online, detail_json, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, 'ON_SALE', 1, '{}', 0, 0)
        "#,
    )
    .bind("uid-1")
    .bind("item-1")
    .bind("Rust Desk Lamp")
    .bind("Warm light")
    .bind("88.00")
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
