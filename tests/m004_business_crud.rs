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
const TEST_BEARER: &str = "m004-business-crud";

#[tokio::test]
async fn community_business_scope_shares_legacy_global_configuration() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_env("ADMIN_USERNAME", "compat-admin");

    let config = AppConfig::from_env().unwrap();
    let user_token = encode_access_token(&config.auth, "17452146585", "user", false).unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_accounts_pool(&config).await;
    let xianyu_pool = connect_xianyu_pool(&config).await;
    seed_item(&xianyu_pool, "admin-owned-uid", "admin-item").await;

    sqlx::query(
        r#"
        INSERT INTO accounts (uid, nick, status, owner_username)
        VALUES
            ('admin-owned-uid', 'Admin 旧账号', 'active', ''),
            ('user-owned-uid', '用户自己的账号', 'active', '17452146585')
        "#,
    )
    .execute(&accounts_pool)
    .await
    .unwrap();
    let admin_keyword_id = sqlx::query(
        "INSERT INTO keywords (cookie_id, keyword, reply, type, enabled, created_at) VALUES ('__global__', 'admin-keyword', 'admin-reply', 'text', 1, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap()
    .last_insert_rowid();
    let admin_item_reply_id = sqlx::query(
        "INSERT INTO item_replies (cookie_id, item_id, reply, type, enabled, created_at) VALUES ('__global__', '', 'admin-default-reply', 'text', 1, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap()
    .last_insert_rowid();
    let admin_card_id = sqlx::query(
        "INSERT INTO cards (cookie_id, card_key, card_content, card_pool_tag, reusable, used, created_at) VALUES ('__global__', 'ADMIN-CARD', 'admin-card-content', 'admin-pool', 1, 0, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap()
    .last_insert_rowid();
    let admin_delivery_rule_id = sqlx::query(
        "INSERT INTO delivery_rules (cookie_id, keyword, item_id, card_pool_tag, card_pool_scope, enabled, created_at) VALUES ('__global__', 'admin-rule', 'admin-item', 'admin-pool', 'global', 1, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap()
    .last_insert_rowid();
    sqlx::query(
        "INSERT INTO ai_reply_settings (cookie_id, api_key, base_url, model_name, custom_prompts, updated_at) VALUES ('__global__', 'sk-admin', 'https://admin.example.com', 'admin-model', '{\"system\":\"admin prompt\"}', 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();

    let app = build_app(state);

    for path in [
        "/api/keywords/__global__",
        "/api/item-replies/__global__",
        "/api/cards/__global__",
        "/api/delivery-rules/__global__",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header("authorization", format!("Bearer {user_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let payload = json_body(response).await;
        assert_eq!(payload.as_array().map(Vec::len), Some(1), "{path}");
    }

    sqlx::query(
        "INSERT INTO cards (cookie_id, card_key, card_content, card_pool_tag, reusable, used, created_at) VALUES ('__global__', 'ADMIN-CARD-2', 'admin-card-content-2', 'admin-pool', 1, 0, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();

    let ai_response = app
        .clone()
        .oneshot(
            Request::get("/api/ai-settings/__global__")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ai_response.status(), StatusCode::OK);
    let ai_payload = json_body(ai_response).await;
    assert_eq!(ai_payload["apiKey"], "sk-admin");

    let local_account_scope = app
        .clone()
        .oneshot(
            Request::get("/api/keywords/admin-owned-uid")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(local_account_scope.status(), StatusCode::OK);

    for request in [
        Request::put(format!("/api/keywords/{admin_keyword_id}"))
            .header("authorization", format!("Bearer {user_token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "reply": "hacked" }).to_string()))
            .unwrap(),
        Request::delete(format!("/api/keywords/{admin_keyword_id}"))
            .header("authorization", format!("Bearer {user_token}"))
            .body(Body::empty())
            .unwrap(),
        Request::put(format!("/api/item-replies/{admin_item_reply_id}"))
            .header("authorization", format!("Bearer {user_token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "reply": "hacked" }).to_string()))
            .unwrap(),
        Request::delete(format!("/api/item-replies/{admin_item_reply_id}"))
            .header("authorization", format!("Bearer {user_token}"))
            .body(Body::empty())
            .unwrap(),
        Request::delete(format!("/api/cards/{admin_card_id}"))
            .header("authorization", format!("Bearer {user_token}"))
            .body(Body::empty())
            .unwrap(),
        Request::put(format!("/api/delivery-rules/{admin_delivery_rule_id}"))
            .header("authorization", format!("Bearer {user_token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "keyword": "hacked" }).to_string()))
            .unwrap(),
        Request::delete(format!("/api/delivery-rules/{admin_delivery_rule_id}"))
            .header("authorization", format!("Bearer {user_token}"))
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    }

    let create_user_global = app
        .clone()
        .oneshot(
            Request::post("/api/keywords/__global__")
                .header("authorization", format!("Bearer {user_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "keyword": "user-keyword",
                        "reply": "user-reply",
                        "type": "text",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_user_global.status(), StatusCode::OK);

    let create_user_global_item_reply = app
        .clone()
        .oneshot(
            Request::post("/api/item-replies/__global__")
                .header("authorization", format!("Bearer {user_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "",
                        "reply": "user-default-reply",
                        "type": "text",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_user_global_item_reply.status(), StatusCode::OK);

    let user_global_list = app
        .clone()
        .oneshot(
            Request::get("/api/keywords/__global__")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let user_global_payload = json_body(user_global_list).await;
    assert_eq!(user_global_payload.as_array().map(Vec::len), Some(1));
    assert_eq!(user_global_payload[0]["cookieId"], "__global__");
    assert_eq!(user_global_payload[0]["keyword"], "user-keyword");

    let user_global_item_reply_list = app
        .clone()
        .oneshot(
            Request::get("/api/item-replies/__global__")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let user_global_item_reply_payload = json_body(user_global_item_reply_list).await;
    assert_eq!(
        user_global_item_reply_payload.as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(user_global_item_reply_payload[0]["cookieId"], "__global__");
    assert_eq!(
        user_global_item_reply_payload[0]["reply"],
        "user-default-reply"
    );

    let stored_user_global_cookie_id: String =
        sqlx::query_scalar("SELECT cookie_id FROM keywords WHERE keyword = 'user-keyword'")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(stored_user_global_cookie_id, "__global__");

    let stored_user_global_item_reply_cookie_id: String =
        sqlx::query_scalar("SELECT cookie_id FROM item_replies WHERE reply = 'user-default-reply'")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(stored_user_global_item_reply_cookie_id, "__global__");

    clear_env();
    clear_env_key("ADMIN_USERNAME");
}

#[tokio::test]
async fn community_business_scope_does_not_split_identity_by_auth_headers() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_env("ADMIN_USERNAME", "admin");

    let config = AppConfig::from_env().unwrap();
    let user_token = encode_access_token(&config.auth, "admin", "user", false).unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_accounts_pool(&config).await;

    sqlx::query(
        "INSERT INTO keywords (cookie_id, keyword, reply, type, enabled, created_at) VALUES ('__global__', 'legacy-admin-keyword', 'legacy-admin-reply', 'text', 1, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();

    let app = build_app(state);
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/keywords/__global__")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload.as_array().map(Vec::len), Some(1));
    assert_eq!(payload[0]["keyword"], "legacy-admin-keyword");

    let create_response = app
        .clone()
        .oneshot(
            Request::post("/api/keywords/__global__")
                .header("authorization", format!("Bearer {user_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "keyword": "admin-user-keyword",
                        "reply": "admin-user-reply",
                        "type": "text",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);

    let stored_cookie_id: String =
        sqlx::query_scalar("SELECT cookie_id FROM keywords WHERE keyword = 'admin-user-keyword'")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(stored_cookie_id, "__global__");

    clear_env();
    clear_env_key("ADMIN_USERNAME");
}

#[tokio::test]
async fn business_crud_routes_round_trip_with_python_shapes() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_accounts_pool(&config).await;
    let xianyu_pool = connect_xianyu_pool(&config).await;
    seed_item(&xianyu_pool, "cookie-001", "item-001").await;
    seed_item(&xianyu_pool, "cookie-001", "item-999").await;

    let keyword_create = app
        .clone()
        .oneshot(
            Request::post("/api/keywords/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "keyword": "ship-now",
                        "reply": "ready",
                        "type": "text",
                        "itemId": "item-001",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(keyword_create.status(), StatusCode::OK);
    let keyword_payload = json_body(keyword_create).await;
    assert_eq!(keyword_payload["status"], "ok");
    assert_eq!(keyword_payload["keyword"]["cookieId"], "cookie-001");
    let keyword_id = keyword_payload["keyword"]["id"].as_i64().unwrap();

    let keyword_update = app
        .clone()
        .oneshot(
            Request::put(format!("/api/keywords/{keyword_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "reply": "updated-reply",
                        "enabled": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(keyword_update.status(), StatusCode::OK);
    let keyword_update_payload = json_body(keyword_update).await;
    assert_eq!(keyword_update_payload["keyword"]["reply"], "updated-reply");
    assert_eq!(keyword_update_payload["keyword"]["enabled"], false);

    let keyword_list = app
        .clone()
        .oneshot(
            Request::get("/api/keywords/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(keyword_list.status(), StatusCode::OK);
    let keyword_list_payload = json_body(keyword_list).await;
    assert_eq!(keyword_list_payload.as_array().map(Vec::len), Some(1));

    let global_keyword_create = app
        .clone()
        .oneshot(
            Request::post("/api/keywords/__global__")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "keyword": "global-word",
                        "reply": "global-ready",
                        "type": "text",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_keyword_create.status(), StatusCode::OK);

    let combined_keyword_list = app
        .clone()
        .oneshot(
            Request::get("/api/keywords/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(combined_keyword_list.status(), StatusCode::OK);
    let combined_keyword_payload = json_body(combined_keyword_list).await;
    assert_eq!(combined_keyword_payload.as_array().map(Vec::len), Some(1));
    assert_eq!(combined_keyword_payload[0]["cookieId"], "cookie-001");

    let global_keyword_list = app
        .clone()
        .oneshot(
            Request::get("/api/keywords/__global__")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_keyword_list.status(), StatusCode::OK);
    let global_keyword_payload = json_body(global_keyword_list).await;
    assert_eq!(global_keyword_payload.as_array().map(Vec::len), Some(1));
    assert_eq!(global_keyword_payload[0]["cookieId"], "__global__");

    let item_reply_create = app
        .clone()
        .oneshot(
            Request::post("/api/item-replies/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "item-001",
                        "reply": "hello buyer",
                        "type": "text",
                        "enabled": true,
                        "replyCount": 3
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(item_reply_create.status(), StatusCode::OK);
    let item_reply_payload = json_body(item_reply_create).await;
    assert_eq!(item_reply_payload["itemReply"]["itemId"], "item-001");
    assert_eq!(item_reply_payload["itemReply"]["replyCount"], 3);
    let item_reply_id = item_reply_payload["itemReply"]["id"].as_i64().unwrap();

    let item_reply_update = app
        .clone()
        .oneshot(
            Request::put(format!("/api/item-replies/{item_reply_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "reply": "hello buyer again",
                        "replyCount": 5
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(item_reply_update.status(), StatusCode::OK);
    let item_reply_update_payload = json_body(item_reply_update).await;
    assert_eq!(
        item_reply_update_payload["itemReply"]["reply"],
        "hello buyer again"
    );
    assert_eq!(item_reply_update_payload["itemReply"]["replyCount"], 5);

    let global_default_create = app
        .clone()
        .oneshot(
            Request::post("/api/item-replies/__global__")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "",
                        "reply": "https://example.com/default.png",
                        "type": "image",
                        "enabled": true,
                        "replyCount": 2
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_default_create.status(), StatusCode::OK);
    let global_default_payload = json_body(global_default_create).await;
    assert_eq!(
        global_default_payload["itemReply"]["cookieId"],
        "__global__"
    );
    assert_eq!(global_default_payload["itemReply"]["itemId"], "");
    assert_eq!(global_default_payload["itemReply"]["type"], "image");
    assert_eq!(global_default_payload["itemReply"]["replyCount"], 2);

    let combined_reply_list = app
        .clone()
        .oneshot(
            Request::get("/api/item-replies/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(combined_reply_list.status(), StatusCode::OK);
    let combined_reply_payload = json_body(combined_reply_list).await;
    assert_eq!(combined_reply_payload.as_array().map(Vec::len), Some(1));
    assert_eq!(combined_reply_payload[0]["cookieId"], "cookie-001");
    assert_eq!(combined_reply_payload[0]["type"], "text");
    assert_eq!(combined_reply_payload[0]["replyCount"], 5);

    let global_item_bound = app
        .clone()
        .oneshot(
            Request::post("/api/item-replies/__global__")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "item-999",
                        "reply": "bad-shape",
                        "type": "text",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_item_bound.status(), StatusCode::OK);
    let global_item_reply_payload = json_body(global_item_bound).await;
    assert_eq!(
        global_item_reply_payload["itemReply"]["cookieId"],
        "__global__"
    );
    assert_eq!(global_item_reply_payload["itemReply"]["itemId"], "item-999");
    assert_eq!(global_item_reply_payload["itemReply"]["reply"], "bad-shape");

    let global_reply_list = app
        .clone()
        .oneshot(
            Request::get("/api/item-replies/__global__")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_reply_list.status(), StatusCode::OK);
    let global_reply_payload = json_body(global_reply_list).await;
    let global_reply_items = global_reply_payload.as_array().cloned().unwrap_or_default();
    assert_eq!(global_reply_items.len(), 2);
    assert!(global_reply_items
        .iter()
        .all(|item| item["cookieId"] == Value::String("__global__".to_string())));
    assert!(global_reply_items
        .iter()
        .any(|item| item["itemId"] == Value::String(String::new())));
    assert!(global_reply_items
        .iter()
        .any(|item| item["itemId"] == Value::String("item-999".to_string())));

    let cards_bulk_create = app
        .clone()
        .oneshot(
            Request::post("/api/cards/bulk")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "cookieId": "cookie-001",
                        "cards": [
                            {
                                "cardKey": "CARD-001",
                                "cardContent": "content-001",
                                "cardPoolTag": "pool-a",
                                "reusable": false
                            }
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cards_bulk_create.status(), StatusCode::OK);
    let cards_payload = json_body(cards_bulk_create).await;
    assert_eq!(cards_payload["status"], "ok");
    assert_eq!(cards_payload["cards"].as_array().map(Vec::len), Some(1));
    assert_eq!(cards_payload["cards"][0]["reusable"], false);

    let global_cards_bulk_create = app
        .clone()
        .oneshot(
            Request::post("/api/cards/bulk")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "cookieId": "__global__",
                        "cards": [
                            {
                                "cardKey": "GLOBAL-FIXED",
                                "cardContent": "global reusable content",
                                "cardPoolTag": "global-pool",
                                "reusable": true
                            }
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_cards_bulk_create.status(), StatusCode::OK);
    let global_cards_payload = json_body(global_cards_bulk_create).await;
    assert_eq!(global_cards_payload["cards"][0]["cookieId"], "__global__");
    assert_eq!(global_cards_payload["cards"][0]["reusable"], true);

    let invalid_rule = app
        .clone()
        .oneshot(
            Request::post("/api/delivery-rules/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "item-001",
                        "cardPoolTag": "missing-pool",
                        "cardPoolScope": "global",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_rule.status(), StatusCode::BAD_REQUEST);

    let valid_rule = app
        .clone()
        .oneshot(
            Request::post("/api/delivery-rules/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "item-001",
                        "cardPoolTag": "pool-a",
                        "cardPoolScope": "account",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(valid_rule.status(), StatusCode::OK);
    let valid_rule_payload = json_body(valid_rule).await;
    assert_eq!(valid_rule_payload["deliveryRule"]["cardPoolTag"], "pool-a");
    assert_eq!(
        valid_rule_payload["deliveryRule"]["cardPoolScope"],
        "account"
    );

    let global_valid_rule = app
        .clone()
        .oneshot(
            Request::post("/api/delivery-rules/__global__")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "item-001",
                        "keyword": "激活码",
                        "cardPoolTag": "global-pool",
                        "cardPoolScope": "global",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_valid_rule.status(), StatusCode::OK);
    let global_rule_payload = json_body(global_valid_rule).await;
    assert_eq!(
        global_rule_payload["deliveryRule"]["cookieId"],
        "__global__"
    );
    assert_eq!(
        global_rule_payload["deliveryRule"]["cardPoolScope"],
        "global"
    );

    let combined_rule_list = app
        .clone()
        .oneshot(
            Request::get("/api/delivery-rules/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(combined_rule_list.status(), StatusCode::OK);
    let combined_rule_payload = json_body(combined_rule_list).await;
    assert_eq!(combined_rule_payload.as_array().map(Vec::len), Some(2));
    assert_eq!(combined_rule_payload[1]["cookieId"], "__global__");

    let stored_counts: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM keywords),
            (SELECT COUNT(*) FROM item_replies),
            (SELECT COUNT(*) FROM cards),
            (SELECT COUNT(*) FROM delivery_rules)
        "#,
    )
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(stored_counts, (2, 3, 2, 2));

    let global_rule_scope: String =
        sqlx::query_scalar("SELECT card_pool_scope FROM delivery_rules WHERE cookie_id = ?")
            .bind("__global__")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(global_rule_scope, "global");

    clear_env();
}

#[tokio::test]
async fn account_default_reply_create_resets_default_reply_history_only_for_default_rules() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state.clone());
    let accounts_pool = connect_accounts_pool(&config).await;
    let xianyu_pool = connect_xianyu_pool(&config).await;
    seed_item(&xianyu_pool, "cookie-001", "item-001").await;

    sqlx::query(
        "INSERT INTO default_reply_records (cookie_id, chat_id, item_id, reply_type, created_at) VALUES ('cookie-001', 'session-a', '', 'default', 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();

    let default_reply_create = app
        .clone()
        .oneshot(
            Request::post("/api/item-replies/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "",
                        "reply": "new default reply",
                        "type": "text",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(default_reply_create.status(), StatusCode::OK);

    let cleared_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM default_reply_records WHERE cookie_id = 'cookie-001' AND item_id = '' AND reply_type = 'default'",
    )
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(cleared_count, 0);

    sqlx::query(
        "INSERT INTO default_reply_records (cookie_id, chat_id, item_id, reply_type, created_at) VALUES ('cookie-001', 'session-b', '', 'default', 2)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();

    let item_reply_create = app
        .clone()
        .oneshot(
            Request::post("/api/item-replies/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "itemId": "item-001",
                        "reply": "item-only reply",
                        "type": "text",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(item_reply_create.status(), StatusCode::OK);

    let preserved_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM default_reply_records WHERE cookie_id = 'cookie-001' AND item_id = '' AND reply_type = 'default'",
    )
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(preserved_count, 1);

    clear_env();
}

#[tokio::test]
async fn deleting_last_account_reply_source_turns_off_auto_reply_for_that_account() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state);
    let accounts_pool = connect_accounts_pool(&config).await;

    sqlx::query(
        "INSERT INTO accounts (uid, nick, status, owner_username, extra) VALUES ('cookie-001', '账号一', 'active', '', '{\"autoReply\":true}')",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ai_reply_settings (cookie_id, ai_enabled, auto_reply_enabled, auto_ship_enabled, updated_at) VALUES ('cookie-001', 0, 1, 0, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();
    let item_reply_id = sqlx::query(
        "INSERT INTO item_replies (cookie_id, item_id, reply, type, enabled, auto_recall, task_group_id, created_at) VALUES ('cookie-001', '', '默认回复', 'text', 1, 0, 'task-1', 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap()
    .last_insert_rowid();

    let response = app
        .clone()
        .oneshot(
            Request::delete(format!("/api/item-replies/{item_reply_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let extra_raw: String =
        sqlx::query_scalar("SELECT extra FROM accounts WHERE uid = 'cookie-001'")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    let extra: Value = serde_json::from_str(&extra_raw).unwrap();
    assert_eq!(extra["autoReply"], json!(false));

    let auto_reply_enabled: i64 = sqlx::query_scalar(
        "SELECT auto_reply_enabled FROM ai_reply_settings WHERE cookie_id = 'cookie-001'",
    )
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(auto_reply_enabled, 0);

    clear_env();
}

#[tokio::test]
async fn deleting_item_reply_keeps_auto_reply_on_when_other_reply_sources_still_exist() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state);
    let accounts_pool = connect_accounts_pool(&config).await;

    sqlx::query(
        "INSERT INTO accounts (uid, nick, status, owner_username, extra) VALUES ('cookie-002', '账号二', 'active', '', '{\"autoReply\":true}')",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ai_reply_settings (cookie_id, ai_enabled, auto_reply_enabled, auto_ship_enabled, updated_at) VALUES ('cookie-002', 0, 1, 0, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO keywords (cookie_id, keyword, reply, type, enabled, created_at) VALUES ('cookie-002', '在吗', '在的', 'text', 1, 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap();
    let item_reply_id = sqlx::query(
        "INSERT INTO item_replies (cookie_id, item_id, reply, type, enabled, auto_recall, task_group_id, created_at) VALUES ('cookie-002', '', '默认回复', 'text', 1, 0, 'task-2', 1)",
    )
    .execute(&accounts_pool)
    .await
    .unwrap()
    .last_insert_rowid();

    let response = app
        .clone()
        .oneshot(
            Request::delete(format!("/api/item-replies/{item_reply_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let extra_raw: String =
        sqlx::query_scalar("SELECT extra FROM accounts WHERE uid = 'cookie-002'")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    let extra: Value = serde_json::from_str(&extra_raw).unwrap();
    assert_eq!(extra["autoReply"], json!(true));

    let auto_reply_enabled: i64 = sqlx::query_scalar(
        "SELECT auto_reply_enabled FROM ai_reply_settings WHERE cookie_id = 'cookie-002'",
    )
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(auto_reply_enabled, 1);

    clear_env();
}

#[tokio::test]
async fn scoped_card_claim_reuses_fixed_cards_and_consumes_one_time_cards() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_accounts_pool(&config).await;

    state
        .storage()
        .bulk_create_cards(
            "__global__",
            &[xianyu_rust_backend_full::models::business::CardCreateItem {
                card_key: "GLOBAL-FIXED".to_string(),
                card_content: "fixed content".to_string(),
                card_pool_tag: "global-pool".to_string(),
                reusable: true,
            }],
        )
        .await
        .unwrap();
    state
        .storage()
        .bulk_create_cards(
            "cookie-001",
            &[
                xianyu_rust_backend_full::models::business::CardCreateItem {
                    card_key: "EMPTY-001".to_string(),
                    card_content: "   ".to_string(),
                    card_pool_tag: "account-pool".to_string(),
                    reusable: false,
                },
                xianyu_rust_backend_full::models::business::CardCreateItem {
                    card_key: "ONCE-001".to_string(),
                    card_content: "one time content".to_string(),
                    card_pool_tag: "account-pool".to_string(),
                    reusable: false,
                },
            ],
        )
        .await
        .unwrap();

    let fixed_first = state
        .storage()
        .claim_next_card_by_pool_scoped("cookie-001", "global", "global-pool", "order-1")
        .await
        .unwrap()
        .expect("fixed card");
    let fixed_second = state
        .storage()
        .claim_next_card_by_pool_scoped("cookie-001", "global", "global-pool", "order-2")
        .await
        .unwrap()
        .expect("fixed card should be reusable");
    assert_eq!(fixed_first.id, fixed_second.id);
    assert_eq!(fixed_second.used, false);

    let once_first = state
        .storage()
        .claim_next_card_by_pool_scoped("cookie-001", "account", "account-pool", "order-3")
        .await
        .unwrap()
        .expect("one time card");
    assert_eq!(once_first.card_key, "ONCE-001");
    assert_eq!(once_first.card_content, "one time content");
    assert_eq!(once_first.used, true);
    let once_second = state
        .storage()
        .claim_next_card_by_pool_scoped("cookie-001", "account", "account-pool", "order-4")
        .await
        .unwrap();
    assert!(once_second.is_none());

    let used_counts: (i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*) FROM cards WHERE card_key = 'GLOBAL-FIXED' AND used = 0),
            (SELECT COUNT(*) FROM cards WHERE card_key = 'ONCE-001' AND used = 1)",
    )
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(used_counts, (1, 1));

    let empty_card_used: i64 =
        sqlx::query_scalar("SELECT used FROM cards WHERE card_key = 'EMPTY-001'")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(empty_card_used, 0);

    clear_env();
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

async fn connect_accounts_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .unwrap()
}

async fn connect_xianyu_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.xianyu_db_path.display()
    ))
    .await
    .unwrap()
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

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "m004-business-secret");
    set_env("API_TOKEN", TEST_BEARER);
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("API_TOKEN");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
