use std::{env, sync::Mutex};

use serde_json::{json, Map, Value};
use tempfile::TempDir;
use xianyu_rust_backend_full::{config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn qr_owner_binding_is_persisted_as_primary() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_feishu_channel(&state).await;

    let binding = state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel_id,
            "owner-a",
            "ou_primary",
            "主用户",
            "oc_primary_chat",
            "p2p",
            "om_primary",
            "evt_primary",
            "primary",
            "qr_owner",
            None,
            None,
            true,
        )
        .await
        .unwrap();

    assert_eq!(binding.role, "primary");
    assert_eq!(binding.joined_via, "qr_owner");
    assert!(binding.enabled);

    clear_env();
}

#[tokio::test]
async fn invite_code_can_be_created_and_consumed_once() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_feishu_channel(&state).await;

    let invite = state
        .storage()
        .create_feishu_collaborator_invite(
            channel_id,
            "owner-a",
            "ABC123",
            "ou_primary",
            "主用户",
            1_800.0,
        )
        .await
        .unwrap();

    let consumed = state
        .storage()
        .consume_feishu_collaborator_invite(channel_id, "ABC123", "ou_collab")
        .await
        .unwrap()
        .expect("invite should be consumed");

    assert_eq!(invite.code, "ABC123");
    assert_eq!(consumed.status, "used");
    assert_eq!(consumed.used_by_sender_id, "ou_collab");

    let second = state
        .storage()
        .consume_feishu_collaborator_invite(channel_id, "ABC123", "ou_other")
        .await
        .unwrap();
    assert!(second.is_none());

    clear_env();
}

async fn create_feishu_channel(state: &AppState) -> i64 {
    state
        .storage()
        .create_notification_channel_for_owner(
            "owner-a",
            "Feishu",
            "feishu",
            &Map::from_iter([
                ("app_id".to_string(), Value::String("cli-a".to_string())),
                ("app_secret".to_string(), Value::String("sec-a".to_string())),
                ("domain".to_string(), Value::String("feishu".to_string())),
                ("allowed_operator_ids".to_string(), json!(["ou_primary"])),
            ]),
            true,
        )
        .await
        .unwrap()
        .id
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "feishu-collaborator-secret");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
