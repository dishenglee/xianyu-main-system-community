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
async fn task_routes_cover_crud_lifecycle_and_delete_queue_baselines() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;

    seed_account_with_owner(&accounts_pool, "uid-1", "admin", "Seller One").await;
    seed_comment_target(
        &xianyu_pool,
        "",
        "item-1",
        "Vintage Lamp",
        "seller-1",
        "seller-a",
    )
    .await;
    seed_comment_target(
        &xianyu_pool,
        "",
        "item-2",
        "Desk Chair",
        "seller-2",
        "seller-b",
    )
    .await;

    let create_response = app
        .clone()
        .oneshot(
            Request::post("/api/tasks")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "account_uids": ["uid-1"],
                        "target_item_ids": ["item-1", "item-2"],
                        "title": "night shift",
                        "comment_templates": ["nice item"],
                        "publish_interval_min_seconds": 5,
                        "publish_interval_max_seconds": 8,
                        "delete_interval_min_seconds": 30,
                        "delete_interval_max_seconds": 60,
                        "delete_after_seconds": 600,
                        "enable_delete_comments": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let create_status = create_response.status();
    let create_body = create_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let create_payload: Value = serde_json::from_slice(&create_body).unwrap();
    assert_eq!(
        create_status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&create_body)
    );
    let task_id = create_payload["task_id"].as_str().unwrap().to_string();
    assert_eq!(create_payload["status"], "draft");
    assert_eq!(create_payload["targets"].as_array().map(Vec::len), Some(2));

    let list_response = app
        .clone()
        .oneshot(
            Request::get("/api/tasks?page=1&page_size=20")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = json_body(list_response).await;
    assert_eq!(list_payload["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(list_payload["pagination"]["total"], 1);

    let update_response = app
        .clone()
        .oneshot(
            Request::patch(format!("/api/tasks/{task_id}"))
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "night shift v2",
                        "comment_templates": ["nice item", "still available?"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);
    let update_payload = json_body(update_response).await;
    assert_eq!(update_payload["title"], "night shift v2");
    assert_eq!(
        update_payload["comment_templates"].as_array().map(Vec::len),
        Some(2)
    );

    let detail_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/tasks/{task_id}"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail_response.status(), StatusCode::OK);
    let detail_payload = json_body(detail_response).await;
    assert_eq!(detail_payload["attempt_summary"]["total"], 0);
    assert_eq!(detail_payload["delete_job_summary"]["total"], 0);

    let start_response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/tasks/{task_id}/start"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::OK);
    let start_payload = json_body(start_response).await;
    assert_eq!(start_payload["status"], "ok");
    assert_eq!(start_payload["task"]["status"], "waiting_delete");

    let progress_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/tasks/{task_id}/progress"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(progress_response.status(), StatusCode::OK);
    let progress_payload = json_body(progress_response).await;
    assert_eq!(progress_payload["status"], "waiting_delete");
    assert_eq!(progress_payload["completed_targets"], 2);
    assert_eq!(progress_payload["successful_targets"], 2);
    assert_eq!(progress_payload["percent"], 100.0);

    let logs_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/tasks/{task_id}/logs?limit=50&offset=0"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs_response.status(), StatusCode::OK);
    let logs_payload = json_body(logs_response).await;
    assert_eq!(logs_payload["pagination"]["total"], 3);
    assert_eq!(logs_payload["items"].as_array().map(Vec::len), Some(3));

    let attempts_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/tasks/{task_id}/attempts?limit=20&offset=0"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(attempts_response.status(), StatusCode::OK);
    let attempts_payload = json_body(attempts_response).await;
    assert_eq!(attempts_payload["pagination"]["total"], 2);
    assert_eq!(attempts_payload["items"][0]["success"], true);

    let delete_jobs_response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/tasks/delete-jobs?task_id={task_id}&page=1&page_size=20"
            ))
            .header("authorization", bearer(&admin_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_jobs_response.status(), StatusCode::OK);
    let delete_jobs_payload = json_body(delete_jobs_response).await;
    let job_id = delete_jobs_payload["items"][0]["id"].as_i64().unwrap();
    assert_eq!(delete_jobs_payload["pagination"]["total"], 2);
    assert_eq!(delete_jobs_payload["items"][0]["status"], "pending");

    let cancel_job_response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/tasks/delete-jobs/{job_id}/cancel"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cancel_job_response.status(), StatusCode::OK);
    let cancel_job_payload = json_body(cancel_job_response).await;
    assert_eq!(cancel_job_payload["job"]["status"], "cancelled");

    let retry_job_response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/tasks/delete-jobs/{job_id}/retry"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(retry_job_response.status(), StatusCode::OK);
    let retry_job_payload = json_body(retry_job_response).await;
    assert_eq!(retry_job_payload["job"]["status"], "pending");

    let pause_response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/tasks/{task_id}/pause"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pause_response.status(), StatusCode::OK);
    let pause_payload = json_body(pause_response).await;
    assert_eq!(pause_payload["task"]["status"], "paused");

    let delete_response = app
        .clone()
        .oneshot(
            Request::delete(format!("/api/tasks/{task_id}"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);
    let delete_payload = json_body(delete_response).await;
    assert_eq!(delete_payload["status"], "ok");
}

#[tokio::test]
#[ignore = "legacy tenant-isolation assertions are obsolete in the single-user community edition"]
async fn regular_user_task_routes_do_not_expose_admin_tasks_or_accounts() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();
    let user_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;

    seed_account_with_owner(&accounts_pool, "uid-user", "user-a", "User Seller").await;
    seed_account_with_owner(&accounts_pool, "uid-admin", "admin", "Admin Seller").await;
    seed_comment_target(
        &xianyu_pool,
        "user-a",
        "item-1",
        "Vintage Lamp",
        "seller-1",
        "seller-a",
    )
    .await;
    seed_comment_target(
        &xianyu_pool,
        "",
        "item-1",
        "Vintage Lamp",
        "seller-1",
        "seller-a",
    )
    .await;
    seed_comment_target(
        &xianyu_pool,
        "admin",
        "item-1",
        "Vintage Lamp",
        "seller-1",
        "seller-a",
    )
    .await;

    let user_create_response = app
        .clone()
        .oneshot(
            Request::post("/api/tasks")
                .header("authorization", bearer(&user_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "account_uids": ["uid-user"],
                        "target_item_ids": ["item-1"],
                        "title": "user task",
                        "comment_templates": ["hello"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(user_create_response.status(), StatusCode::OK);
    let user_task_id = json_body(user_create_response).await["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let admin_create_response = app
        .clone()
        .oneshot(
            Request::post("/api/tasks")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "account_uids": ["uid-admin"],
                        "target_item_ids": ["item-1"],
                        "title": "admin task",
                        "comment_templates": ["hello"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_create_response.status(), StatusCode::OK);
    let admin_task_id = json_body(admin_create_response).await["task_id"]
        .as_str()
        .unwrap()
        .to_string();

    let list_response = app
        .clone()
        .oneshot(
            Request::get("/api/tasks?page=1&page_size=20")
                .header("authorization", bearer(&user_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = json_body(list_response).await;
    assert_eq!(list_payload["pagination"]["total"], 1);
    assert_eq!(list_payload["items"][0]["task_id"], user_task_id);

    let admin_detail_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/tasks/{admin_task_id}"))
                .header("authorization", bearer(&user_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_detail_response.status(), StatusCode::NOT_FOUND);

    for (method, path) in [
        ("GET", format!("/api/tasks/{admin_task_id}/progress")),
        ("GET", format!("/api/tasks/{admin_task_id}/logs")),
        ("GET", format!("/api/tasks/{admin_task_id}/attempts")),
        ("POST", format!("/api/tasks/{admin_task_id}/start")),
        ("POST", format!("/api/tasks/{admin_task_id}/pause")),
        ("DELETE", format!("/api/tasks/{admin_task_id}")),
    ] {
        let request = match method {
            "GET" => Request::get(path),
            "POST" => Request::post(path),
            "DELETE" => Request::delete(path),
            other => panic!("unsupported method {other}"),
        }
        .header("authorization", bearer(&user_token))
        .body(Body::empty())
        .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{method} admin task route must be hidden from regular users"
        );
    }

    let admin_start_response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/tasks/{admin_task_id}/start"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_start_response.status(), StatusCode::OK);

    let admin_delete_jobs_response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/tasks/delete-jobs?task_id={admin_task_id}&page=1&page_size=20"
            ))
            .header("authorization", bearer(&admin_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_delete_jobs_response.status(), StatusCode::OK);
    let admin_delete_jobs_payload = json_body(admin_delete_jobs_response).await;
    let admin_job_id = admin_delete_jobs_payload["items"][0]["id"]
        .as_i64()
        .unwrap();
    assert_eq!(admin_delete_jobs_payload["pagination"]["total"], 1);

    let user_delete_jobs_response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/tasks/delete-jobs?task_id={admin_task_id}&page=1&page_size=20"
            ))
            .header("authorization", bearer(&user_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(user_delete_jobs_response.status(), StatusCode::OK);
    let user_delete_jobs_payload = json_body(user_delete_jobs_response).await;
    assert_eq!(user_delete_jobs_payload["pagination"]["total"], 0);
    assert_eq!(
        user_delete_jobs_payload["items"].as_array().map(Vec::len),
        Some(0)
    );

    for path in [
        format!("/api/tasks/delete-jobs/{admin_job_id}/cancel"),
        format!("/api/tasks/delete-jobs/{admin_job_id}/retry"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("authorization", bearer(&user_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "admin delete job route must be hidden from regular users"
        );
    }

    let cross_account_create_response = app
        .clone()
        .oneshot(
            Request::post("/api/tasks")
                .header("authorization", bearer(&user_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "account_uids": ["uid-admin"],
                        "target_item_ids": ["item-1"],
                        "title": "cross account task",
                        "comment_templates": ["hello"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        cross_account_create_response.status(),
        StatusCode::NOT_FOUND
    );
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
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

async fn seed_account_with_owner(pool: &SqlitePool, uid: &str, owner: &str, nick: &str) {
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
        VALUES (?, ?, ?, '', '', '', '', 'active', 0, 0, 0, 0, 0, 0, '', '', '{}', 92, 0, '', 0, 0, 0, 0)
        "#,
    )
    .bind(uid)
    .bind(owner)
    .bind(nick)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_comment_target(
    pool: &SqlitePool,
    owner_username: &str,
    item_id: &str,
    title: &str,
    seller_id: &str,
    seller_nick: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO comment_targets (
            owner_username,
            item_id,
            title,
            price,
            pic_url,
            item_url,
            seller_id,
            seller_nick,
            source,
            status,
            selected_at,
            created_at,
            updated_at
        )
        VALUES (?, ?, ?, '99', '', '', ?, ?, 'search', 'pending', 0, 0, 0)
        "#,
    )
    .bind(owner_username)
    .bind(item_id)
    .bind(title)
    .bind(seller_id)
    .bind(seller_nick)
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
