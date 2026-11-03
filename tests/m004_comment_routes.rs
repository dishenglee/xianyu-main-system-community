use std::{env, path::Path, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "route-token";

#[tokio::test]
async fn comment_routes_cover_targets_library_and_candidates() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let xianyu_pool = connect_xianyu_pool(&config).await;
    seed_account(&config, "cookie-001").await;
    seed_account(&config, "cookie-002").await;

    let batch_add = app
        .clone()
        .oneshot(
            Request::post("/api/comment-targets/batch-add")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "items": [
                            {
                                "item_id": "item-001",
                                "title": "Vintage Lamp",
                                "price": "88",
                                "seller_nick": "seller-a"
                            },
                            {
                                "item_id": "item-002",
                                "title": "Desk Chair",
                                "price": "128",
                                "seller_nick": "seller-b"
                            }
                        ]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(batch_add.status(), StatusCode::OK);
    let batch_payload = json_body(batch_add).await;
    assert_eq!(batch_payload["status"], "ok");
    assert_eq!(batch_payload["requested"], 2);
    assert_eq!(batch_payload["added"], 2);

    let targets_list = app
        .clone()
        .oneshot(
            Request::get("/api/comment-targets?page=1&page_size=20")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(targets_list.status(), StatusCode::OK);
    let targets_payload = json_body(targets_list).await;
    assert_eq!(targets_payload["items"].as_array().map(Vec::len), Some(2));
    assert_eq!(targets_payload["pagination"]["total"], 2);

    seed_snapshot(
        &xianyu_pool,
        "cookie-001",
        "item-001",
        "物流很快，包装很好",
        "物流",
        "good",
        0.91,
    )
    .await;
    seed_snapshot(
        &xianyu_pool,
        "cookie-001",
        "item-002",
        "服务很好，下次再来",
        "服务",
        "good",
        0.88,
    )
    .await;

    let candidates_list = app
        .clone()
        .oneshot(
            Request::get("/api/comment-candidates?uid=cookie-001&page=1&page_size=50")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(candidates_list.status(), StatusCode::OK);
    let candidates_payload = json_body(candidates_list).await;
    assert_eq!(
        candidates_payload["items"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(candidates_payload["summary"]["total"], 2);

    let export_dir = temp_dir.path().join("comment_exports");
    let export_response = app
        .clone()
        .oneshot(
            Request::post("/api/comment-candidates/export")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "cookie-001",
                        "directory": export_dir.display().to_string(),
                        "file_name": "comment-candidates.txt"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(export_response.status(), StatusCode::OK);
    let export_payload = json_body(export_response).await;
    assert_eq!(export_payload["exported"], 2);
    let export_file = export_payload["file_path"].as_str().unwrap().to_string();
    assert!(Path::new(&export_file).exists());

    let library_manual_create = app
        .clone()
        .oneshot(
            Request::post("/api/comment-library")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "content": "手动录入评论"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(library_manual_create.status(), StatusCode::OK);
    let library_manual_payload = json_body(library_manual_create).await;
    assert_eq!(library_manual_payload["imported"], 1);
    let content_hash = library_manual_payload["item"]["content_hash"]
        .as_str()
        .unwrap()
        .to_string();

    let library_import_candidates = app
        .clone()
        .oneshot(
            Request::post("/api/comment-library/import/candidates")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "cookie-001"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(library_import_candidates.status(), StatusCode::OK);
    let library_import_payload = json_body(library_import_candidates).await;
    assert_eq!(library_import_payload["imported"], 2);

    let library_toggle = app
        .clone()
        .oneshot(
            Request::post("/api/comment-library/enabled")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "content_hash": content_hash,
                        "enabled": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(library_toggle.status(), StatusCode::OK);
    let library_toggle_payload = json_body(library_toggle).await;
    assert_eq!(library_toggle_payload["item"]["enabled"], false);

    let library_list = app
        .clone()
        .oneshot(
            Request::get("/api/comment-library?page=1&page_size=50")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(library_list.status(), StatusCode::OK);
    let library_list_payload = json_body(library_list).await;
    assert_eq!(
        library_list_payload["items"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(library_list_payload["summary"]["total"], 3);

    let import_response = app
        .clone()
        .oneshot(
            Request::post("/api/comment-candidates/import")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "cookie-002",
                        "file_path": export_file,
                        "category": "服务"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(import_response.status(), StatusCode::OK);
    let import_payload = json_body(import_response).await;
    assert_eq!(import_payload["imported"], 2);

    clear_env();
}

async fn seed_snapshot(
    pool: &SqlitePool,
    uid: &str,
    item_id: &str,
    content: &str,
    category: &str,
    quality_label: &str,
    quality_score: f64,
) {
    let snapshot_time = 1_715_000_000.0_f64;
    let content_hash = format!("{uid}-{item_id}");
    sqlx::query(
        r#"
        INSERT INTO item_comment_snapshots (
            uid, item_id, source_api, source_type, source_comment_key,
            comment_id, author_id, author_nick, content, content_hash,
            published_at, category, quality_score, quality_label, score_breakdown_json,
            raw_json, snapshot_time, accepted_for_txt, txt_exported_at, txt_export_path,
            txt_imported_at, created_at, updated_at
        )
        VALUES (?, ?, 'seed-api', 'seed', ?, '', '', '', ?, ?, '', ?, ?, ?, '{}', '{}', ?, 0, 0, '', 0, ?, ?)
        "#,
    )
    .bind(uid)
    .bind(item_id)
    .bind(format!("{uid}:{item_id}:{content_hash}"))
    .bind(content)
    .bind(content_hash)
    .bind(category)
    .bind(quality_score)
    .bind(quality_label)
    .bind(snapshot_time)
    .bind(snapshot_time)
    .bind(snapshot_time)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_account(config: &AppConfig, uid: &str) {
    let pool = connect_accounts_pool(config).await;
    sqlx::query(
        r#"
        INSERT INTO accounts (uid, owner_username, nick, status, token, cookies, created_at, updated_at)
        VALUES (?, '', ?, 'active', '', '{}', unixepoch('subsec'), unixepoch('subsec'))
        "#,
    )
    .bind(uid)
    .bind(format!("账号 {uid}"))
    .execute(&pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

async fn connect_xianyu_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.xianyu_db_path.display()
    ))
    .await
    .unwrap()
}

async fn connect_accounts_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "m004-comment-secret");
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
