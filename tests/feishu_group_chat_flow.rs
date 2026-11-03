use std::{
    env,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Map, Value};
use tempfile::TempDir;
use tokio::time::sleep;
use xianyu_rust_backend_full::{
    config::AppConfig,
    notifications::{
        feishu_group_router::{
            append_group_message, find_or_create_group_for_session,
            repair_group_metadata_for_event, FindOrCreateGroupOutcome,
        },
        feishu_native::{
            FeishuNativeCallFuture, FeishuNativeRequest, FeishuNativeResponse, FeishuNativeRuntime,
            FeishuNativeTransport,
        },
    },
    routes::accounts::LiveMessageEvent,
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn creates_and_reuses_feishu_session_group_mapping() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_feishu_channel(&state).await;

    let created = state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            "owner-a",
            "uid-a",
            "session-a",
            "peer-a",
            "客户甲",
            "item-a",
            "原始商品",
            "https://img.example/item-a.png",
            "88.00",
            "oc_group_a",
            "会话群 A",
            "ou_operator_a",
            "active",
            1_718_000_100.0,
        )
        .await
        .unwrap();

    let fetched_by_session = state
        .storage()
        .get_feishu_session_group_chat_by_session(channel_id, "owner-a", "uid-a", "session-a")
        .await
        .unwrap()
        .expect("mapping should be created");
    assert_eq!(fetched_by_session.id, created.id);
    assert_eq!(fetched_by_session.feishu_chat_id, "oc_group_a");
    assert_eq!(fetched_by_session.customer_name, "客户甲");
    assert_eq!(fetched_by_session.status, "active");
    assert_eq!(fetched_by_session.closed_at, 0.0);

    sleep(Duration::from_millis(20)).await;

    let updated = state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            "owner-a",
            "uid-a",
            "session-a",
            "peer-a",
            "客户乙",
            "item-a",
            "更新商品",
            "https://img.example/item-b.png",
            "108.50",
            "oc_group_b",
            "会话群 B",
            "ou_operator_b",
            "active",
            1_718_000_200.0,
        )
        .await
        .unwrap();

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.customer_name, "客户乙");
    assert_eq!(updated.feishu_chat_id, "oc_group_b");
    assert_eq!(updated.feishu_operator_sender_id, "ou_operator_b");
    assert_eq!(updated.last_message_ts, 1_718_000_200.0);
    assert!(updated.updated_at >= created.updated_at);

    let fetched_by_chat = state
        .storage()
        .get_feishu_session_group_chat_by_chat_id(channel_id, "oc_group_b")
        .await
        .unwrap()
        .expect("mapping should be reachable by chat id");
    assert_eq!(fetched_by_chat.id, created.id);
    assert_eq!(fetched_by_chat.item_title, "更新商品");
    assert_eq!(fetched_by_chat.item_price, "108.50");

    state
        .storage()
        .update_feishu_session_group_status(
            channel_id,
            "session-a",
            "closed",
            "会话群 B 已关闭",
            Some(1_718_000_300.0),
        )
        .await
        .unwrap();

    let closed = state
        .storage()
        .get_feishu_session_group_chat_by_session(channel_id, "owner-a", "uid-a", "session-a")
        .await
        .unwrap()
        .expect("mapping should still exist after close");
    assert_eq!(closed.status, "closed");
    assert_eq!(closed.feishu_chat_name, "会话群 B 已关闭");
    assert_eq!(closed.closed_at, 1_718_000_300.0);

    sleep(Duration::from_millis(20)).await;

    let refreshed_after_close = state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            "owner-a",
            "uid-a",
            "session-a",
            "peer-a",
            "客户丙",
            "item-a",
            "关闭后刷新商品",
            "https://img.example/item-c.png",
            "128.00",
            "oc_group_b",
            "会话群 B 刷新",
            "ou_operator_c",
            "active",
            1_718_000_400.0,
        )
        .await
        .unwrap();
    assert_eq!(refreshed_after_close.id, created.id);
    assert_eq!(refreshed_after_close.status, "closed");
    assert_eq!(refreshed_after_close.closed_at, 1_718_000_300.0);
    assert_eq!(refreshed_after_close.customer_name, "客户丙");
    assert_eq!(refreshed_after_close.item_title, "关闭后刷新商品");
    assert_eq!(refreshed_after_close.last_message_ts, 1_718_000_400.0);

    assert!(state
        .storage()
        .delete_notification_channel_for_owner(channel_id, "owner-a")
        .await
        .unwrap());

    assert!(state
        .storage()
        .get_feishu_session_group_chat_by_session(channel_id, "owner-a", "uid-a", "session-a")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM feishu_session_group_chats WHERE channel_id = ?"
        )
        .bind(channel_id)
        .fetch_one(state.storage().notifications())
        .await
        .unwrap(),
        0
    );

    clear_env();
}

#[tokio::test]
async fn rejects_duplicate_non_empty_feishu_chat_ids_per_channel() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_feishu_channel(&state).await;

    state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            "owner-a",
            "uid-a",
            "session-a",
            "peer-a",
            "客户甲",
            "item-a",
            "商品 A",
            "https://img.example/item-a.png",
            "88.00",
            "oc_group_dup",
            "会话群 A",
            "ou_operator_a",
            "active",
            1_718_010_100.0,
        )
        .await
        .unwrap();

    let duplicate_chat_id = state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            "owner-a",
            "uid-a",
            "session-b",
            "peer-b",
            "客户乙",
            "item-b",
            "商品 B",
            "https://img.example/item-b.png",
            "99.00",
            "oc_group_dup",
            "会话群 B",
            "ou_operator_b",
            "active",
            1_718_010_200.0,
        )
        .await;
    assert!(
        duplicate_chat_id.is_err(),
        "non-empty Feishu group ids should stay unique per channel"
    );

    let empty_a = state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            "owner-a",
            "uid-a",
            "session-empty-a",
            "peer-empty-a",
            "客户丙",
            "item-c",
            "商品 C",
            "https://img.example/item-c.png",
            "58.00",
            "",
            "",
            "",
            "active",
            1_718_010_300.0,
        )
        .await
        .unwrap();
    let empty_b = state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            "owner-a",
            "uid-a",
            "session-empty-b",
            "peer-empty-b",
            "客户丁",
            "item-d",
            "商品 D",
            "https://img.example/item-d.png",
            "68.00",
            "",
            "",
            "",
            "active",
            1_718_010_400.0,
        )
        .await
        .unwrap();
    assert_ne!(empty_a.id, empty_b.id);
    assert!(
        state
            .storage()
            .get_feishu_session_group_chat_by_chat_id(channel_id, "   ")
            .await
            .unwrap()
            .is_none(),
        "empty Feishu group ids must not reverse-resolve to an arbitrary row"
    );

    clear_env();
}

#[tokio::test]
async fn returns_latest_operator_activity_for_owner() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_feishu_channel(&state).await;

    state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            "owner-a",
            "ou_operator_old",
            "旧值班",
            "oc_group_old",
            "evt-old",
        )
        .await
        .unwrap();

    sleep(Duration::from_millis(20)).await;

    state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            "owner-b",
            "ou_other_owner",
            "其他店主",
            "oc_group_other",
            "evt-other",
        )
        .await
        .unwrap();

    sleep(Duration::from_millis(20)).await;

    let latest = state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            "owner-a",
            "ou_operator_new",
            "新值班",
            "oc_group_new",
            "evt-new",
        )
        .await
        .unwrap();

    let fetched = state
        .storage()
        .get_latest_feishu_operator_activity(channel_id, "owner-a")
        .await
        .unwrap()
        .expect("latest operator activity should exist");
    assert_eq!(fetched.id, latest.id);
    assert_eq!(fetched.sender_id, "ou_operator_new");
    assert_eq!(fetched.sender_name, "新值班");
    assert_eq!(fetched.last_chat_id, "oc_group_new");
    assert_eq!(fetched.last_event_id, "evt-new");
    assert!(fetched.last_active_at >= latest.last_active_at);

    let owner_b = state
        .storage()
        .get_latest_feishu_operator_activity(channel_id, "owner-b")
        .await
        .unwrap()
        .expect("other owner activity should stay isolated");
    assert_eq!(owner_b.sender_id, "ou_other_owner");

    assert!(state
        .storage()
        .delete_notification_channel_for_owner(channel_id, "owner-a")
        .await
        .unwrap());

    assert!(state
        .storage()
        .get_latest_feishu_operator_activity(channel_id, "owner-a")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM feishu_operator_activity WHERE channel_id = ?"
        )
        .bind(channel_id)
        .fetch_one(state.storage().notifications())
        .await
        .unwrap(),
        0
    );

    clear_env();
}

#[tokio::test]
async fn upserts_existing_operator_activity_for_same_sender() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_feishu_channel(&state).await;

    let first = state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            "owner-a",
            "ou_operator_same",
            "值班一号",
            "oc_group_first",
            "evt-first",
        )
        .await
        .unwrap();

    sleep(Duration::from_millis(20)).await;

    let second = state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            "owner-a",
            "ou_operator_same",
            "值班一号（更新）",
            "oc_group_second",
            "evt-second",
        )
        .await
        .unwrap();

    assert_eq!(second.id, first.id);
    assert_eq!(second.sender_name, "值班一号（更新）");
    assert_eq!(second.last_chat_id, "oc_group_second");
    assert_eq!(second.last_event_id, "evt-second");
    assert!(second.updated_at >= first.updated_at);
    assert!(second.last_active_at >= first.last_active_at);

    let row_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM feishu_operator_activity
        WHERE channel_id = ? AND owner_username = ? AND sender_id = ?
        "#,
    )
    .bind(channel_id)
    .bind("owner-a")
    .bind("ou_operator_same")
    .fetch_one(state.storage().notifications())
    .await
    .unwrap();
    assert_eq!(row_count, 1);

    clear_env();
}

#[tokio::test]
async fn first_real_session_message_creates_group_and_second_message_reuses_it() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingGroupTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-a", "七七书屋11").await;
    let channel = create_started_feishu_channel(&state).await;
    seed_latest_operator(&state, channel.id, "owner-a", "ou_operator_1", "Alice").await;

    let first = sample_live_event("session-a", "buyer-a", "客户甲", "MacBook Air", "你好");
    let second = sample_live_event("session-a", "buyer-a", "客户甲", "MacBook Air", "还在吗");

    let created =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &first)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };
    append_group_message(&state, &channel, &created, &first)
        .await
        .unwrap();

    let reused =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &second)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };

    assert_eq!(created.feishu_chat_id, reused.feishu_chat_id);
    assert_eq!(reused.session_id, "session-a");
    let calls = transport.calls();
    assert_eq!(calls[0].method, "chat.create");
    assert_eq!(calls[1].method, "message.send");
    assert_eq!(calls[2].method, "message.send");
    assert_eq!(calls[1].params["msg_type"], "interactive");
    assert_eq!(calls[2].params["msg_type"], "text");
    assert_eq!(
        calls[2].params["content"]["text"],
        Value::String("客户：你好".to_string())
    );

    clear_env();
}

#[tokio::test]
async fn group_create_passes_customer_avatar_into_chat_create_request() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingGroupTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-a", "七七书屋11").await;
    let channel = create_started_feishu_channel(&state).await;
    seed_latest_operator(&state, channel.id, "owner-a", "ou_operator_1", "Alice").await;

    let mut first = sample_live_event(
        "session-avatar",
        "buyer-avatar",
        "客户头像",
        "MacBook Air",
        "你好",
    );
    first.peer_logo = "https://img.example/buyer-avatar.png".to_string();

    let _created =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &first)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };

    let calls = transport.calls();
    assert_eq!(calls[0].method, "chat.create");
    assert_eq!(
        calls[0].params["avatar_url"],
        Value::String("https://img.example/buyer-avatar.png".to_string())
    );

    clear_env();
}

#[tokio::test]
async fn second_message_with_customer_avatar_patches_group_avatar() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingGroupTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-a", "七七书屋11").await;
    let channel = create_started_feishu_channel(&state).await;
    seed_latest_operator(&state, channel.id, "owner-a", "ou_operator_1", "Alice").await;

    let first = sample_live_event(
        "session-avatar-patch",
        "buyer-avatar-patch",
        "客户补头像",
        "MacBook Air",
        "你好",
    );
    let mut second = sample_live_event(
        "session-avatar-patch",
        "buyer-avatar-patch",
        "客户补头像",
        "MacBook Air",
        "头像来了",
    );
    second.peer_logo = "https://img.example/buyer-avatar-later.png".to_string();

    let created =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &first)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };
    append_group_message(&state, &channel, &created, &first)
        .await
        .unwrap();

    let reused =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &second)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };
    append_group_message(&state, &channel, &reused, &second)
        .await
        .unwrap();

    let calls = transport.calls();
    assert_eq!(calls[3].method, "chat.patch");
    assert_eq!(
        calls[3].params["avatar_url"],
        Value::String("https://img.example/buyer-avatar-later.png".to_string())
    );
    assert_eq!(calls[4].method, "message.send");

    clear_env();
}

#[tokio::test]
async fn second_message_with_item_title_repairs_group_name_and_refreshes_main_card() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingGroupTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-a", "七七书屋11").await;
    let channel = create_started_feishu_channel(&state).await;
    seed_latest_operator(&state, channel.id, "owner-a", "ou_operator_1", "Alice").await;

    let first = sample_live_event(
        "session-title-repair",
        "buyer-title-repair",
        "客户补商品",
        "",
        "你好",
    );
    let second = sample_live_event(
        "session-title-repair",
        "buyer-title-repair",
        "客户补商品",
        "MacBook Air",
        "商品补全了",
    );

    let created =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &first)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };
    append_group_message(&state, &channel, &created, &first)
        .await
        .unwrap();

    let reused =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &second)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };
    append_group_message(&state, &channel, &reused, &second)
        .await
        .unwrap();

    let refreshed = state
        .storage()
        .get_feishu_session_group_chat_by_session(
            channel.id,
            "owner-a",
            "seller-a",
            "session-title-repair",
        )
        .await
        .unwrap()
        .expect("mapping should exist");
    assert_eq!(refreshed.item_title, "MacBook Air");
    assert_eq!(refreshed.feishu_chat_name, "客户补商品｜MacBook Air");

    let calls = transport.calls();
    assert_eq!(calls[3].method, "chat.patch");
    assert_eq!(calls[3].params["name"], "客户补商品｜MacBook Air");
    assert_eq!(calls[4].method, "message.send");
    assert_eq!(calls[4].params["msg_type"], "interactive");
    assert!(
        !calls[4].params["content"].to_string().contains("未知商品"),
        "refreshed main card should no longer show 未知商品"
    );
    assert_eq!(calls[5].method, "message.send");
    assert_eq!(
        calls[5].params["content"]["text"],
        Value::String("客户：商品补全了".to_string())
    );

    clear_env();
}

#[tokio::test]
async fn hydration_repair_updates_existing_group_name_avatar_and_main_card() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingGroupTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-a", "七七书屋11").await;
    let channel = create_started_feishu_channel(&state).await;
    seed_latest_operator(&state, channel.id, "owner-a", "ou_operator_1", "Alice").await;

    let first = sample_live_event(
        "session-hydrate-repair",
        "buyer-hydrate",
        "客户补资料",
        "",
        "在吗",
    );

    let created =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &first)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };
    append_group_message(&state, &channel, &created, &first)
        .await
        .unwrap();

    let mut hydrated = first.clone();
    hydrated.item_title = "ThinkPad X1 Carbon".to_string();
    hydrated.peer_logo = "https://img.example/hydrated-avatar.png".to_string();
    repair_group_metadata_for_event(&state, "seller-a", &hydrated)
        .await
        .unwrap();

    let refreshed = state
        .storage()
        .get_feishu_session_group_chat_by_session(
            channel.id,
            "owner-a",
            "seller-a",
            "session-hydrate-repair",
        )
        .await
        .unwrap()
        .expect("mapping should exist");
    assert_eq!(refreshed.item_title, "ThinkPad X1 Carbon");
    assert_eq!(refreshed.feishu_chat_name, "客户补资料｜ThinkPad X1 Carbon");

    let calls = transport.calls();
    assert_eq!(calls[3].method, "chat.patch");
    assert_eq!(
        calls[3].params["avatar_url"],
        Value::String("https://img.example/hydrated-avatar.png".to_string())
    );
    assert_eq!(calls[3].params["name"], "客户补资料｜ThinkPad X1 Carbon");
    assert_eq!(calls[4].method, "message.send");
    assert_eq!(calls[4].params["msg_type"], "interactive");
    assert!(
        !calls[4].params["content"].to_string().contains("未知商品"),
        "hydration repair main card should no longer show 未知商品"
    );

    clear_env();
}

#[tokio::test]
async fn first_real_session_message_creates_group_from_bound_sender_when_operator_activity_missing()
{
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingGroupTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-a", "七七书屋11").await;
    let channel = create_started_feishu_channel(&state).await;
    seed_bound_operator(&state, channel.id, "owner-a", "ou_bound_1", "Alice").await;

    let first = sample_live_event(
        "session-bound",
        "buyer-bound",
        "客户乙",
        "iPhone 15",
        "你好",
    );

    let created =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &first)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Existing { mapping }
            | FindOrCreateGroupOutcome::Created { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };

    assert_eq!(created.feishu_operator_sender_id, "ou_bound_1");
    let calls = transport.calls();
    assert_eq!(calls[0].method, "chat.create");

    clear_env();
}

#[tokio::test]
async fn group_creation_includes_all_enabled_owner_members() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingGroupTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-a", "七七书屋11").await;
    let channel = create_started_feishu_channel(&state).await;

    seed_bound_member(
        &state,
        channel.id,
        "owner-a",
        "ou_primary_1",
        "老板",
        "primary",
        true,
    )
    .await;
    seed_bound_member(
        &state,
        channel.id,
        "owner-a",
        "ou_collab_1",
        "客服A",
        "collaborator",
        true,
    )
    .await;
    seed_bound_member(
        &state,
        channel.id,
        "owner-a",
        "ou_collab_disabled",
        "客服B",
        "collaborator",
        false,
    )
    .await;
    seed_latest_operator(&state, channel.id, "owner-a", "ou_collab_1", "客服A").await;

    let event = sample_live_event(
        "session-team",
        "buyer-team",
        "客户协作",
        "iPhone 15 Pro Max",
        "在吗",
    );

    let mapping =
        match find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &event)
            .await
            .unwrap()
        {
            FindOrCreateGroupOutcome::Created { mapping }
            | FindOrCreateGroupOutcome::Existing { mapping } => mapping,
            FindOrCreateGroupOutcome::Fallback { reason } => {
                panic!("unexpected fallback: {reason}")
            }
        };

    assert_eq!(mapping.feishu_operator_sender_id, "ou_collab_1");
    let calls = transport.calls();
    assert_eq!(calls[0].method, "chat.create");
    let mut member_ids = calls[0].params["member_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    member_ids.sort();
    assert_eq!(
        member_ids,
        vec!["ou_collab_1".to_string(), "ou_primary_1".to_string()]
    );

    clear_env();
}

#[tokio::test]
async fn group_create_failure_marks_failed_and_returns_fallback() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingGroupTransport::with_failed_chat_create();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel = create_started_feishu_channel(&state).await;
    seed_latest_operator(&state, channel.id, "owner-a", "ou_operator_1", "Alice").await;
    let event = sample_live_event(
        "session-fail",
        "buyer-fail",
        "客户乙",
        "iPhone 13",
        "有人吗",
    );

    let outcome = find_or_create_group_for_session(&state, &channel, "owner-a", "seller-a", &event)
        .await
        .unwrap();

    assert!(matches!(outcome, FindOrCreateGroupOutcome::Fallback { .. }));
    let failed = state
        .storage()
        .get_feishu_session_group_chat_by_session(channel.id, "owner-a", "seller-a", "session-fail")
        .await
        .unwrap()
        .expect("failed mapping should still be persisted");
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.feishu_chat_id, "");

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
            ]),
            true,
        )
        .await
        .unwrap()
        .id
}

async fn create_started_feishu_channel(
    state: &AppState,
) -> xianyu_rust_backend_full::models::notifications::NotificationChannel {
    let channel_id = create_feishu_channel(state).await;
    state
        .storage()
        .get_notification_channel(channel_id)
        .await
        .unwrap()
        .expect("channel should exist")
}

async fn seed_account(state: &AppState, uid: &str, nick: &str) {
    sqlx::query(
        "INSERT INTO accounts (uid, nick, status, owner_username, created_at, updated_at) VALUES (?, ?, 'active', 'owner-a', unixepoch('subsec'), unixepoch('subsec'))",
    )
    .bind(uid)
    .bind(nick)
    .execute(state.storage().accounts())
    .await
    .unwrap();
}

async fn seed_latest_operator(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    sender_name: &str,
) {
    state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            owner_username,
            sender_id,
            sender_name,
            "",
            "evt-seed",
        )
        .await
        .unwrap();
}

async fn seed_bound_operator(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    sender_name: &str,
) {
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            owner_username,
            sender_id,
            sender_name,
            "oc_binding",
            "p2p",
            "om-binding",
            "evt-binding",
            true,
        )
        .await
        .unwrap();
}

async fn seed_bound_member(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    sender_name: &str,
    role: &str,
    enabled: bool,
) {
    state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel_id,
            owner_username,
            sender_id,
            sender_name,
            "oc_binding",
            "p2p",
            "om-binding",
            "evt-binding",
            role,
            "manual_bind",
            None,
            None,
            enabled,
        )
        .await
        .unwrap();
}

fn sample_live_event(
    session_id: &str,
    peer_uid: &str,
    peer_nick: &str,
    item_title: &str,
    text: &str,
) -> LiveMessageEvent {
    LiveMessageEvent {
        session_id: session_id.to_string(),
        session_type: 1,
        ts: 1_719_225_600,
        peer_uid: peer_uid.to_string(),
        peer_nick: peer_nick.to_string(),
        item_id: format!("item-{session_id}"),
        item_title: item_title.to_string(),
        item_pic: "https://img.example/item.png".to_string(),
        text: text.to_string(),
        preview: text.to_string(),
        kind: "text".to_string(),
        ..LiveMessageEvent::default()
    }
}

#[derive(Clone)]
struct RecordingGroupTransport {
    calls: Arc<Mutex<Vec<FeishuNativeRequest>>>,
    fail_chat_create: bool,
}

impl Default for RecordingGroupTransport {
    fn default() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_chat_create: false,
        }
    }
}

impl RecordingGroupTransport {
    fn with_failed_chat_create() -> Self {
        Self {
            fail_chat_create: true,
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<FeishuNativeRequest> {
        self.calls.lock().unwrap().clone()
    }
}

impl FeishuNativeTransport for RecordingGroupTransport {
    fn call(&self, request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        self.calls.lock().unwrap().push(request.clone());
        let fail_chat_create = self.fail_chat_create;
        Box::pin(async move {
            if request.method == "chat.create" && fail_chat_create {
                return Err(
                    xianyu_rust_backend_full::notifications::feishu_native::FeishuNativeError::Api(
                        "chat.create HTTP 500".to_string(),
                    ),
                );
            }
            Ok(FeishuNativeResponse::success(
                match request.method.as_str() {
                    "chat.create" => serde_json::json!({
                        "status": "created",
                        "chat_id": "oc_group_session_a",
                        "name": request.params["name"],
                    }),
                    "message.send" => serde_json::json!({
                        "status": "sent",
                        "message_id": "om_group_card",
                        "fell_back": false,
                    }),
                    "chat.patch" => serde_json::json!({
                        "status": "ok",
                        "chat_id": request.params["chat_id"],
                        "name": request.params["name"],
                    }),
                    other => panic!("unexpected method {other}"),
                },
            ))
        })
    }
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "feishu-group-chat-secret");
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
