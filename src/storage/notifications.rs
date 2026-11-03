use serde_json::{Map, Value};
use sqlx::{sqlite::SqliteRow, Row};

use crate::models::notifications::{
    normalize_notification_type, FeishuCollaboratorInvite, FeishuImReplyRoute,
    FeishuOperatorActivity, FeishuPendingAction, FeishuSessionGroupChat, FeishuUserBinding,
    NotificationBinding, NotificationChannel, NotificationRuntime,
};

use super::{parse_json_map, StorageError, StorageState};

impl StorageState {
    /// List every enabled channel whose JSON config has
    /// `bind_all_accounts: true` — these channels receive IM events from
    /// EVERY active uid without needing per-account binding rows.
    ///
    /// We fall back to fetch-all-then-filter-in-Rust because the JSON column
    /// is stored as TEXT and boolean stringification has historically been
    /// inconsistent across SQLx revisions (true vs 1 vs "true"). One pass
    /// over a small set is cheap and avoids that footgun entirely.
    pub async fn list_bind_all_notification_channels(
        &self,
    ) -> Result<Vec<NotificationChannel>, StorageError> {
        let all = self.list_notification_channels().await?;
        Ok(filter_bind_all_channels(all))
    }

    pub async fn list_bind_all_notification_channels_for_owner(
        &self,
        owner_username: &str,
    ) -> Result<Vec<NotificationChannel>, StorageError> {
        let all = self
            .list_notification_channels_for_owner(owner_username)
            .await?;
        Ok(filter_bind_all_channels(all))
    }

    pub async fn list_bind_all_notification_channels_for_uid(
        &self,
        uid: &str,
    ) -> Result<Vec<NotificationChannel>, StorageError> {
        let owner = self.notification_owner_for_uid(uid).await?;
        self.list_bind_all_notification_channels_for_owner(&owner)
            .await
    }

    pub async fn list_notification_channels(
        &self,
    ) -> Result<Vec<NotificationChannel>, StorageError> {
        self.list_notification_channels_where(None).await
    }

    pub async fn list_notification_channels_for_owner(
        &self,
        owner_username: &str,
    ) -> Result<Vec<NotificationChannel>, StorageError> {
        self.list_notification_channels_where(Some(owner_username))
            .await
    }

    async fn list_notification_channels_where(
        &self,
        owner_username: Option<&str>,
    ) -> Result<Vec<NotificationChannel>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, type, COALESCE(owner_username, '') AS owner_username, config, enabled, created_at, updated_at
            FROM notification_channels
            WHERE (? IS NULL OR owner_username = ?)
            ORDER BY updated_at DESC, id DESC
            "#,
        )
        .bind(owner_username)
        .bind(owner_username)
        .fetch_all(&self.notifications)
        .await?;

        let mut channels = Vec::with_capacity(rows.len());
        for row in rows {
            let channel_id = row.get::<i64, _>("id");
            let raw_type = row.get::<String, _>("type");
            let Some(channel_type) = normalize_notification_type(&raw_type) else {
                continue;
            };
            channels.push(NotificationChannel {
                id: channel_id,
                name: row.get::<String, _>("name"),
                channel_type,
                owner_username: row.get::<String, _>("owner_username"),
                config: parse_json_map(row.get::<String, _>("config").as_str()),
                enabled: row.get::<i64, _>("enabled") != 0,
                created_at: row.get::<f64, _>("created_at"),
                updated_at: row.get::<f64, _>("updated_at"),
                runtime: self.get_notification_runtime(channel_id).await?,
            });
        }

        Ok(channels)
    }

    pub async fn get_notification_channel(
        &self,
        channel_id: i64,
    ) -> Result<Option<NotificationChannel>, StorageError> {
        self.get_notification_channel_where(channel_id, None).await
    }

    pub async fn get_notification_channel_for_owner(
        &self,
        channel_id: i64,
        owner_username: &str,
    ) -> Result<Option<NotificationChannel>, StorageError> {
        self.get_notification_channel_where(channel_id, Some(owner_username))
            .await
    }

    async fn get_notification_channel_where(
        &self,
        channel_id: i64,
        owner_username: Option<&str>,
    ) -> Result<Option<NotificationChannel>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, type, COALESCE(owner_username, '') AS owner_username, config, enabled, created_at, updated_at
            FROM notification_channels
            WHERE id = ? AND (? IS NULL OR owner_username = ?)
            "#,
        )
        .bind(channel_id)
        .bind(owner_username)
        .bind(owner_username)
        .fetch_optional(&self.notifications)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let channel_id = row.get::<i64, _>("id");
        let raw_type = row.get::<String, _>("type");
        let Some(channel_type) = normalize_notification_type(&raw_type) else {
            return Ok(None);
        };

        Ok(Some(NotificationChannel {
            id: channel_id,
            name: row.get::<String, _>("name"),
            channel_type,
            owner_username: row.get::<String, _>("owner_username"),
            config: parse_json_map(row.get::<String, _>("config").as_str()),
            enabled: row.get::<i64, _>("enabled") != 0,
            created_at: row.get::<f64, _>("created_at"),
            updated_at: row.get::<f64, _>("updated_at"),
            runtime: self.get_notification_runtime(channel_id).await?,
        }))
    }

    pub async fn create_notification_channel(
        &self,
        name: &str,
        channel_type: &str,
        config: &Map<String, serde_json::Value>,
        enabled: bool,
    ) -> Result<NotificationChannel, StorageError> {
        self.create_notification_channel_for_owner("", name, channel_type, config, enabled)
            .await
    }

    pub async fn create_notification_channel_for_owner(
        &self,
        owner_username: &str,
        name: &str,
        channel_type: &str,
        config: &Map<String, serde_json::Value>,
        enabled: bool,
    ) -> Result<NotificationChannel, StorageError> {
        let normalized_type = normalize_notification_type(channel_type).ok_or_else(|| {
            StorageError::Io(std::io::Error::other(format!(
                "unsupported notification channel type: {channel_type}"
            )))
        })?;
        let normalized_config = normalize_notification_config_keys(&normalized_type, config);
        let serialized_config = serde_json::to_string(&normalized_config)
            .map_err(|error| StorageError::Io(std::io::Error::other(error)))?;

        let cursor = sqlx::query(
            r#"
            INSERT INTO notification_channels (name, type, owner_username, config, enabled, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, unixepoch('subsec'), unixepoch('subsec'))
            "#,
        )
        .bind(name)
        .bind(normalized_type)
        .bind(owner_username.trim())
        .bind(serialized_config)
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .execute(&self.notifications)
        .await?;

        self.get_notification_channel(cursor.last_insert_rowid())
            .await?
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::other(
                    "created notification channel missing",
                ))
            })
    }

    pub async fn update_notification_channel(
        &self,
        channel_id: i64,
        name: &str,
        channel_type: &str,
        config: &Map<String, serde_json::Value>,
        enabled: bool,
    ) -> Result<Option<NotificationChannel>, StorageError> {
        self.update_notification_channel_for_owner(
            channel_id,
            "",
            name,
            channel_type,
            config,
            enabled,
        )
        .await
    }

    pub async fn update_notification_channel_for_owner(
        &self,
        channel_id: i64,
        owner_username: &str,
        name: &str,
        channel_type: &str,
        config: &Map<String, serde_json::Value>,
        enabled: bool,
    ) -> Result<Option<NotificationChannel>, StorageError> {
        let normalized_type = normalize_notification_type(channel_type).ok_or_else(|| {
            StorageError::Io(std::io::Error::other(format!(
                "unsupported notification channel type: {channel_type}"
            )))
        })?;
        let normalized_config = normalize_notification_config_keys(&normalized_type, config);
        let serialized_config = serde_json::to_string(&normalized_config)
            .map_err(|error| StorageError::Io(std::io::Error::other(error)))?;

        let result = sqlx::query(
            r#"
            UPDATE notification_channels
            SET name = ?, type = ?, config = ?, enabled = ?, updated_at = unixepoch('subsec')
            WHERE id = ? AND owner_username = ?
            "#,
        )
        .bind(name)
        .bind(normalized_type)
        .bind(serialized_config)
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(channel_id)
        .bind(owner_username.trim())
        .execute(&self.notifications)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.get_notification_channel_for_owner(channel_id, owner_username)
            .await
    }

    pub async fn delete_notification_channel(&self, channel_id: i64) -> Result<bool, StorageError> {
        self.delete_notification_channel_for_owner(channel_id, "")
            .await
    }

    pub async fn delete_notification_channel_for_owner(
        &self,
        channel_id: i64,
        owner_username: &str,
    ) -> Result<bool, StorageError> {
        if self
            .get_notification_channel_for_owner(channel_id, owner_username)
            .await?
            .is_none()
        {
            return Ok(false);
        }

        sqlx::query("DELETE FROM notification_channel_runtime WHERE channel_id = ?")
            .bind(channel_id)
            .execute(&self.notifications)
            .await?;
        sqlx::query("DELETE FROM account_notification_bindings WHERE channel_id = ?")
            .bind(channel_id)
            .execute(&self.notifications)
            .await?;
        sqlx::query("DELETE FROM feishu_user_bindings WHERE channel_id = ?")
            .bind(channel_id)
            .execute(&self.notifications)
            .await?;
        sqlx::query("DELETE FROM feishu_im_reply_routes WHERE channel_id = ?")
            .bind(channel_id)
            .execute(&self.notifications)
            .await?;
        sqlx::query("DELETE FROM feishu_session_group_chats WHERE channel_id = ?")
            .bind(channel_id)
            .execute(&self.notifications)
            .await?;
        sqlx::query("DELETE FROM feishu_operator_activity WHERE channel_id = ?")
            .bind(channel_id)
            .execute(&self.notifications)
            .await?;
        sqlx::query("DELETE FROM feishu_pending_actions WHERE channel_id = ?")
            .bind(channel_id)
            .execute(&self.notifications)
            .await?;

        let result =
            sqlx::query("DELETE FROM notification_channels WHERE id = ? AND owner_username = ?")
                .bind(channel_id)
                .bind(owner_username.trim())
                .execute(&self.notifications)
                .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_notification_runtime(
        &self,
        channel_id: i64,
    ) -> Result<NotificationRuntime, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT channel_id, status, last_error, last_attempt_at, last_success_at, runtime_json, updated_at
            FROM notification_channel_runtime
            WHERE channel_id = ?
            "#,
        )
        .bind(channel_id)
        .fetch_optional(&self.notifications)
        .await?;

        Ok(match row {
            Some(row) => NotificationRuntime {
                channel_id: row.get::<i64, _>("channel_id"),
                status: row.get::<String, _>("status"),
                last_error: row.get::<String, _>("last_error"),
                last_attempt_at: row.get::<f64, _>("last_attempt_at"),
                last_success_at: row.get::<f64, _>("last_success_at"),
                runtime_json: parse_json_map(row.get::<String, _>("runtime_json").as_str()),
                updated_at: row.get::<f64, _>("updated_at"),
            },
            None => NotificationRuntime {
                channel_id,
                ..NotificationRuntime::default()
            },
        })
    }

    pub async fn upsert_notification_runtime(
        &self,
        channel_id: i64,
        status: &str,
        last_error: &str,
        last_attempt_at: Option<f64>,
        last_success_at: Option<f64>,
        runtime_patch: &Map<String, serde_json::Value>,
    ) -> Result<NotificationRuntime, StorageError> {
        let existing = self.get_notification_runtime(channel_id).await?;
        let mut merged_runtime = existing.runtime_json.clone();
        merged_runtime.extend(runtime_patch.clone());
        let serialized_runtime = serde_json::to_string(&merged_runtime)
            .map_err(|error| StorageError::Io(std::io::Error::other(error)))?;

        sqlx::query(
            r#"
            INSERT INTO notification_channel_runtime (
                channel_id,
                status,
                last_error,
                last_attempt_at,
                last_success_at,
                runtime_json,
                updated_at
            )
            VALUES (
                ?,
                ?,
                ?,
                COALESCE(?, unixepoch('subsec')),
                COALESCE(?, 0),
                ?,
                unixepoch('subsec')
            )
            ON CONFLICT(channel_id) DO UPDATE SET
                status = excluded.status,
                last_error = excluded.last_error,
                last_attempt_at = excluded.last_attempt_at,
                last_success_at = excluded.last_success_at,
                runtime_json = excluded.runtime_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(channel_id)
        .bind(status)
        .bind(last_error)
        .bind(last_attempt_at)
        .bind(last_success_at)
        .bind(serialized_runtime)
        .execute(&self.notifications)
        .await?;

        self.get_notification_runtime(channel_id).await
    }

    pub async fn patch_notification_runtime(
        &self,
        channel_id: i64,
        runtime_patch: &Map<String, serde_json::Value>,
    ) -> Result<NotificationRuntime, StorageError> {
        let existing = self.get_notification_runtime(channel_id).await?;
        let status = if existing.status.trim().is_empty() {
            "idle".to_string()
        } else {
            existing.status.clone()
        };

        self.upsert_notification_runtime(
            channel_id,
            &status,
            &existing.last_error,
            Some(existing.last_attempt_at.max(0.0)),
            Some(existing.last_success_at.max(0.0)),
            runtime_patch,
        )
        .await
    }

    pub async fn list_notification_bindings(
        &self,
        uid: Option<&str>,
    ) -> Result<Vec<NotificationBinding>, StorageError> {
        self.list_notification_bindings_for_owner(uid, None).await
    }

    pub async fn list_notification_bindings_for_owner(
        &self,
        uid: Option<&str>,
        owner_username: Option<&str>,
    ) -> Result<Vec<NotificationBinding>, StorageError> {
        let base_query = r#"
            SELECT
                b.id,
                b.uid,
                b.channel_id,
                b.enabled,
                b.created_at,
                b.updated_at,
                c.name AS channel_name,
                c.type AS channel_type,
                c.config AS channel_config,
                c.enabled AS channel_enabled
            FROM account_notification_bindings b
            JOIN notification_channels c ON c.id = b.channel_id
        "#;

        let mut conditions = Vec::new();
        if uid.is_some() {
            conditions.push("b.uid = ?");
        }
        if owner_username.is_some() {
            conditions.push("c.owner_username = ?");
        }
        let where_sql = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let query = format!("{base_query}{where_sql} ORDER BY b.updated_at DESC, b.id DESC");
        let mut query = sqlx::query(&query);
        if let Some(uid) = uid {
            query = query.bind(uid);
        }
        if let Some(owner) = owner_username {
            query = query.bind(owner.trim());
        }
        let rows = query.fetch_all(&self.notifications).await?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let channel_type =
                    normalize_notification_type(&row.get::<String, _>("channel_type"))?;
                Some(NotificationBinding {
                    id: row.get::<i64, _>("id"),
                    uid: row.get::<String, _>("uid"),
                    channel_id: row.get::<i64, _>("channel_id"),
                    enabled: row.get::<i64, _>("enabled") != 0,
                    created_at: row.get::<f64, _>("created_at"),
                    updated_at: row.get::<f64, _>("updated_at"),
                    channel_name: row.get::<String, _>("channel_name"),
                    channel_type,
                    channel_config: parse_json_map(row.get::<String, _>("channel_config").as_str()),
                    channel_enabled: row.get::<i64, _>("channel_enabled") != 0,
                })
            })
            .collect())
    }

    pub async fn set_notification_binding(
        &self,
        uid: &str,
        channel_id: i64,
        enabled: bool,
    ) -> Result<Option<NotificationBinding>, StorageError> {
        self.set_notification_binding_for_owner(uid, channel_id, enabled, None)
            .await
    }

    pub async fn set_notification_binding_for_owner(
        &self,
        uid: &str,
        channel_id: i64,
        enabled: bool,
        owner_username: Option<&str>,
    ) -> Result<Option<NotificationBinding>, StorageError> {
        if self.get_notification_channel(channel_id).await?.is_none() {
            return Ok(None);
        }
        if let Some(owner) = owner_username {
            if self
                .get_notification_channel_for_owner(channel_id, owner)
                .await?
                .is_none()
            {
                return Ok(None);
            }
        }

        sqlx::query(
            r#"
            INSERT INTO account_notification_bindings (uid, channel_id, enabled, created_at, updated_at)
            VALUES (?, ?, ?, unixepoch('subsec'), unixepoch('subsec'))
            ON CONFLICT(uid, channel_id) DO UPDATE SET
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(uid)
        .bind(channel_id)
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .execute(&self.notifications)
        .await?;

        let bindings = self
            .list_notification_bindings_for_owner(Some(uid), owner_username)
            .await?;
        Ok(bindings
            .into_iter()
            .find(|binding| binding.channel_id == channel_id))
    }

    pub async fn delete_notification_binding(&self, binding_id: i64) -> Result<bool, StorageError> {
        self.delete_notification_binding_for_owner(binding_id, None)
            .await
    }

    pub async fn delete_notification_binding_for_owner(
        &self,
        binding_id: i64,
        owner_username: Option<&str>,
    ) -> Result<bool, StorageError> {
        if let Some(owner) = owner_username {
            let exists = sqlx::query(
                r#"
                SELECT 1
                FROM account_notification_bindings b
                JOIN notification_channels c ON c.id = b.channel_id
                WHERE b.id = ? AND c.owner_username = ?
                LIMIT 1
                "#,
            )
            .bind(binding_id)
            .bind(owner.trim())
            .fetch_optional(&self.notifications)
            .await?;
            if exists.is_none() {
                return Ok(false);
            }
        }

        let result = sqlx::query("DELETE FROM account_notification_bindings WHERE id = ?")
            .bind(binding_id)
            .execute(&self.notifications)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_notification_uid_bindings(&self, uid: &str) -> Result<u64, StorageError> {
        self.delete_notification_uid_bindings_for_owner(uid, None)
            .await
    }

    pub async fn delete_notification_uid_bindings_for_owner(
        &self,
        uid: &str,
        owner_username: Option<&str>,
    ) -> Result<u64, StorageError> {
        let result = if let Some(owner) = owner_username {
            sqlx::query(
                r#"
                DELETE FROM account_notification_bindings
                WHERE uid = ?
                  AND channel_id IN (
                    SELECT id FROM notification_channels WHERE owner_username = ?
                  )
                "#,
            )
            .bind(uid)
            .bind(owner.trim())
            .execute(&self.notifications)
            .await?
        } else {
            sqlx::query("DELETE FROM account_notification_bindings WHERE uid = ?")
                .bind(uid)
                .execute(&self.notifications)
                .await?
        };
        Ok(result.rows_affected())
    }

    pub async fn get_feishu_user_binding(
        &self,
        channel_id: i64,
        sender_id: &str,
    ) -> Result<Option<FeishuUserBinding>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, sender_id, sender_name, chat_id,
                   chat_type, source, role, joined_via, invited_by_sender_id,
                   invited_by_sender_name, last_message_id, last_event_id, last_seen_at,
                   enabled, created_at, updated_at
            FROM feishu_user_bindings
            WHERE channel_id = ? AND sender_id = ?
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(sender_id.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_user_binding_from_row(&row)))
    }

    pub async fn get_latest_feishu_user_binding_for_owner(
        &self,
        channel_id: i64,
        owner_username: &str,
    ) -> Result<Option<FeishuUserBinding>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, sender_id, sender_name, chat_id,
                   chat_type, source, role, joined_via, invited_by_sender_id,
                   invited_by_sender_name, last_message_id, last_event_id, last_seen_at,
                   enabled, created_at, updated_at
            FROM feishu_user_bindings
            WHERE channel_id = ? AND owner_username = ? AND enabled = 1
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_user_binding_from_row(&row)))
    }

    pub async fn get_primary_feishu_user_binding_for_owner(
        &self,
        channel_id: i64,
        owner_username: &str,
    ) -> Result<Option<FeishuUserBinding>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, sender_id, sender_name, chat_id,
                   chat_type, source, role, joined_via, invited_by_sender_id,
                   invited_by_sender_name, last_message_id, last_event_id, last_seen_at,
                   enabled, created_at, updated_at
            FROM feishu_user_bindings
            WHERE channel_id = ? AND owner_username = ? AND enabled = 1 AND role = 'primary'
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_user_binding_from_row(&row)))
    }

    pub async fn list_feishu_user_bindings_for_owner(
        &self,
        channel_id: i64,
        owner_username: &str,
    ) -> Result<Vec<FeishuUserBinding>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, sender_id, sender_name, chat_id,
                   chat_type, source, role, joined_via, invited_by_sender_id,
                   invited_by_sender_name, last_message_id, last_event_id, last_seen_at,
                   enabled, created_at, updated_at
            FROM feishu_user_bindings
            WHERE channel_id = ? AND owner_username = ?
            ORDER BY CASE role WHEN 'primary' THEN 0 ELSE 1 END, updated_at DESC, id DESC
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .fetch_all(&self.notifications)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| feishu_user_binding_from_row(&row))
            .collect())
    }

    pub async fn upsert_feishu_user_binding(
        &self,
        channel_id: i64,
        owner_username: &str,
        sender_id: &str,
        sender_name: &str,
        chat_id: &str,
        chat_type: &str,
        message_id: &str,
        event_id: &str,
        enabled: bool,
    ) -> Result<FeishuUserBinding, StorageError> {
        self.upsert_feishu_user_binding_with_role(
            channel_id,
            owner_username,
            sender_id,
            sender_name,
            chat_id,
            chat_type,
            message_id,
            event_id,
            "collaborator",
            "manual_bind",
            None,
            None,
            enabled,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_feishu_user_binding_with_role(
        &self,
        channel_id: i64,
        owner_username: &str,
        sender_id: &str,
        sender_name: &str,
        chat_id: &str,
        chat_type: &str,
        message_id: &str,
        event_id: &str,
        role: &str,
        joined_via: &str,
        invited_by_sender_id: Option<&str>,
        invited_by_sender_name: Option<&str>,
        enabled: bool,
    ) -> Result<FeishuUserBinding, StorageError> {
        let sender_id = sender_id.trim();
        let owner_username = owner_username.trim();
        let role = role.trim();
        let joined_via = joined_via.trim();
        let invited_by_sender_id = invited_by_sender_id.unwrap_or("").trim();
        let invited_by_sender_name = invited_by_sender_name.unwrap_or("").trim();

        if role == "primary" {
            sqlx::query(
                r#"
                UPDATE feishu_user_bindings
                SET role = 'collaborator',
                    updated_at = unixepoch('subsec')
                WHERE channel_id = ?
                  AND sender_id <> ?
                  AND role = 'primary'
                "#,
            )
            .bind(channel_id)
            .bind(sender_id)
            .execute(&self.notifications)
            .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO feishu_user_bindings (
                channel_id,
                owner_username,
                sender_id,
                sender_name,
                chat_id,
                chat_type,
                source,
                role,
                joined_via,
                invited_by_sender_id,
                invited_by_sender_name,
                last_message_id,
                last_event_id,
                last_seen_at,
                enabled,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, 'im_command', ?, ?, ?, ?, ?, ?, unixepoch('subsec'), ?, unixepoch('subsec'), unixepoch('subsec'))
            ON CONFLICT(channel_id, sender_id) DO UPDATE SET
                owner_username = excluded.owner_username,
                sender_name = excluded.sender_name,
                chat_id = excluded.chat_id,
                chat_type = excluded.chat_type,
                source = excluded.source,
                role = excluded.role,
                joined_via = excluded.joined_via,
                invited_by_sender_id = excluded.invited_by_sender_id,
                invited_by_sender_name = excluded.invited_by_sender_name,
                last_message_id = excluded.last_message_id,
                last_event_id = excluded.last_event_id,
                last_seen_at = excluded.last_seen_at,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(channel_id)
        .bind(owner_username)
        .bind(sender_id)
        .bind(sender_name.trim())
        .bind(chat_id.trim())
        .bind(chat_type.trim())
        .bind(role)
        .bind(joined_via)
        .bind(invited_by_sender_id)
        .bind(invited_by_sender_name)
        .bind(message_id.trim())
        .bind(event_id.trim())
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .execute(&self.notifications)
        .await?;

        self.get_feishu_user_binding(channel_id, sender_id)
            .await?
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::other(
                    "upserted Feishu user binding missing",
                ))
            })
    }

    pub async fn create_feishu_collaborator_invite(
        &self,
        channel_id: i64,
        owner_username: &str,
        code: &str,
        created_by_sender_id: &str,
        created_by_sender_name: &str,
        expires_in_seconds: f64,
    ) -> Result<FeishuCollaboratorInvite, StorageError> {
        sqlx::query(
            r#"
            INSERT INTO feishu_collaborator_invites (
                channel_id,
                owner_username,
                code,
                created_by_sender_id,
                created_by_sender_name,
                status,
                expires_at,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, 'active', unixepoch('subsec') + ?, unixepoch('subsec'), unixepoch('subsec'))
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .bind(code.trim())
        .bind(created_by_sender_id.trim())
        .bind(created_by_sender_name.trim())
        .bind(expires_in_seconds)
        .execute(&self.notifications)
        .await?;

        self.get_feishu_collaborator_invite(channel_id, code)
            .await?
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::other(
                    "created Feishu collaborator invite missing",
                ))
            })
    }

    pub async fn get_feishu_collaborator_invite(
        &self,
        channel_id: i64,
        code: &str,
    ) -> Result<Option<FeishuCollaboratorInvite>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, code, created_by_sender_id,
                   created_by_sender_name, status, expires_at, used_by_sender_id,
                   used_at, created_at, updated_at
            FROM feishu_collaborator_invites
            WHERE channel_id = ? AND code = ?
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(code.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_collaborator_invite_from_row(&row)))
    }

    pub async fn list_active_feishu_collaborator_invites(
        &self,
        channel_id: i64,
        owner_username: &str,
    ) -> Result<Vec<FeishuCollaboratorInvite>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, code, created_by_sender_id,
                   created_by_sender_name, status, expires_at, used_by_sender_id,
                   used_at, created_at, updated_at
            FROM feishu_collaborator_invites
            WHERE channel_id = ?
              AND owner_username = ?
              AND status = 'active'
              AND (expires_at <= 0 OR expires_at > unixepoch('subsec'))
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .fetch_all(&self.notifications)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| feishu_collaborator_invite_from_row(&row))
            .collect())
    }

    pub async fn consume_feishu_collaborator_invite(
        &self,
        channel_id: i64,
        code: &str,
        used_by_sender_id: &str,
    ) -> Result<Option<FeishuCollaboratorInvite>, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE feishu_collaborator_invites
            SET status = 'used',
                used_by_sender_id = ?,
                used_at = unixepoch('subsec'),
                updated_at = unixepoch('subsec')
            WHERE channel_id = ?
              AND code = ?
              AND status = 'active'
              AND (expires_at <= 0 OR expires_at > unixepoch('subsec'))
            "#,
        )
        .bind(used_by_sender_id.trim())
        .bind(channel_id)
        .bind(code.trim())
        .execute(&self.notifications)
        .await?;

        if result.rows_affected() == 0 {
            Ok(None)
        } else {
            self.get_feishu_collaborator_invite(channel_id, code).await
        }
    }

    pub async fn touch_feishu_user_binding_seen(
        &self,
        channel_id: i64,
        sender_id: &str,
        sender_name: &str,
        chat_id: &str,
        chat_type: &str,
        message_id: &str,
        event_id: &str,
    ) -> Result<Option<FeishuUserBinding>, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE feishu_user_bindings
            SET sender_name = ?,
                chat_id = ?,
                chat_type = ?,
                last_message_id = ?,
                last_event_id = ?,
                last_seen_at = unixepoch('subsec'),
                updated_at = unixepoch('subsec')
            WHERE channel_id = ? AND sender_id = ?
            "#,
        )
        .bind(sender_name.trim())
        .bind(chat_id.trim())
        .bind(chat_type.trim())
        .bind(message_id.trim())
        .bind(event_id.trim())
        .bind(channel_id)
        .bind(sender_id.trim())
        .execute(&self.notifications)
        .await?;

        if result.rows_affected() == 0 {
            Ok(None)
        } else {
            self.get_feishu_user_binding(channel_id, sender_id).await
        }
    }

    pub async fn delete_feishu_user_binding(
        &self,
        channel_id: i64,
        sender_id: &str,
    ) -> Result<bool, StorageError> {
        let result =
            sqlx::query("DELETE FROM feishu_user_bindings WHERE channel_id = ? AND sender_id = ?")
                .bind(channel_id)
                .bind(sender_id.trim())
                .execute(&self.notifications)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_feishu_im_reply_route(
        &self,
        channel_id: i64,
        feishu_message_id: &str,
    ) -> Result<Option<FeishuImReplyRoute>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, feishu_message_id, chat_id, uid,
                   session_id, peer_uid, customer_name, item_id, item_title,
                   xianyu_message_id, xianyu_upstream_message_id, created_at, expires_at
            FROM feishu_im_reply_routes
            WHERE channel_id = ?
              AND feishu_message_id = ?
              AND (expires_at <= 0 OR expires_at > unixepoch('subsec'))
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(feishu_message_id.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_im_reply_route_from_row(&row)))
    }

    pub async fn get_latest_feishu_im_reply_route_for_chat(
        &self,
        channel_id: i64,
        chat_id: &str,
    ) -> Result<Option<FeishuImReplyRoute>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, feishu_message_id, chat_id, uid,
                   session_id, peer_uid, customer_name, item_id, item_title,
                   xianyu_message_id, xianyu_upstream_message_id, created_at, expires_at
            FROM feishu_im_reply_routes
            WHERE channel_id = ? AND chat_id = ? AND (expires_at <= 0 OR expires_at > unixepoch('subsec'))
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(chat_id.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_im_reply_route_from_row(&row)))
    }

    pub async fn upsert_feishu_im_reply_route(
        &self,
        channel_id: i64,
        owner_username: &str,
        feishu_message_id: &str,
        chat_id: &str,
        uid: &str,
        session_id: &str,
        peer_uid: &str,
        customer_name: &str,
        item_id: &str,
        item_title: &str,
        xianyu_message_id: &str,
        xianyu_upstream_message_id: &str,
    ) -> Result<FeishuImReplyRoute, StorageError> {
        let feishu_message_id = feishu_message_id.trim();
        sqlx::query(
            r#"
            INSERT INTO feishu_im_reply_routes (
                channel_id,
                owner_username,
                feishu_message_id,
                chat_id,
                uid,
                session_id,
                peer_uid,
                customer_name,
                item_id,
                item_title,
                xianyu_message_id,
                xianyu_upstream_message_id,
                created_at,
                expires_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'), unixepoch('subsec') + 604800)
            ON CONFLICT(channel_id, feishu_message_id) DO UPDATE SET
                owner_username = excluded.owner_username,
                chat_id = excluded.chat_id,
                uid = excluded.uid,
                session_id = excluded.session_id,
                peer_uid = excluded.peer_uid,
                customer_name = excluded.customer_name,
                item_id = excluded.item_id,
                item_title = excluded.item_title,
                xianyu_message_id = excluded.xianyu_message_id,
                xianyu_upstream_message_id = excluded.xianyu_upstream_message_id,
                expires_at = excluded.expires_at
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .bind(feishu_message_id)
        .bind(chat_id.trim())
        .bind(uid.trim())
        .bind(session_id.trim())
        .bind(peer_uid.trim())
        .bind(customer_name.trim())
        .bind(item_id.trim())
        .bind(item_title.trim())
        .bind(xianyu_message_id.trim())
        .bind(xianyu_upstream_message_id.trim())
        .execute(&self.notifications)
        .await?;

        self.get_feishu_im_reply_route(channel_id, feishu_message_id)
            .await?
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::other(
                    "upserted Feishu IM reply route missing",
                ))
            })
    }

    pub async fn get_feishu_session_group_chat_by_session(
        &self,
        channel_id: i64,
        owner_username: &str,
        uid: &str,
        session_id: &str,
    ) -> Result<Option<FeishuSessionGroupChat>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, uid, session_id, peer_uid, customer_name,
                   item_id, item_title, item_pic, item_price, feishu_chat_id, feishu_chat_name,
                   feishu_operator_sender_id, status, created_at, updated_at, last_message_ts,
                   closed_at
            FROM feishu_session_group_chats
            WHERE channel_id = ?
              AND owner_username = ?
              AND uid = ?
              AND session_id = ?
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .bind(uid.trim())
        .bind(session_id.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_session_group_chat_from_row(&row)))
    }

    pub async fn get_feishu_session_group_chat_by_chat_id(
        &self,
        channel_id: i64,
        feishu_chat_id: &str,
    ) -> Result<Option<FeishuSessionGroupChat>, StorageError> {
        let feishu_chat_id = feishu_chat_id.trim();
        if feishu_chat_id.is_empty() {
            return Ok(None);
        }

        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, uid, session_id, peer_uid, customer_name,
                   item_id, item_title, item_pic, item_price, feishu_chat_id, feishu_chat_name,
                   feishu_operator_sender_id, status, created_at, updated_at, last_message_ts,
                   closed_at
            FROM feishu_session_group_chats
            WHERE channel_id = ? AND feishu_chat_id = ?
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(feishu_chat_id)
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_session_group_chat_from_row(&row)))
    }

    pub async fn list_active_feishu_session_group_chats_for_owner(
        &self,
        channel_id: i64,
        owner_username: &str,
    ) -> Result<Vec<FeishuSessionGroupChat>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, uid, session_id, peer_uid, customer_name,
                   item_id, item_title, item_pic, item_price, feishu_chat_id, feishu_chat_name,
                   feishu_operator_sender_id, status, created_at, updated_at, last_message_ts,
                   closed_at
            FROM feishu_session_group_chats
            WHERE channel_id = ?
              AND owner_username = ?
              AND status = 'active'
              AND closed_at <= 0
              AND TRIM(COALESCE(feishu_chat_id, '')) <> ''
            ORDER BY updated_at DESC, id DESC
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .fetch_all(&self.notifications)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| feishu_session_group_chat_from_row(&row))
            .collect())
    }

    pub async fn list_active_feishu_session_group_chats_for_uid_session(
        &self,
        uid: &str,
        session_id: &str,
    ) -> Result<Vec<FeishuSessionGroupChat>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, uid, session_id, peer_uid, customer_name,
                   item_id, item_title, item_pic, item_price, feishu_chat_id, feishu_chat_name,
                   feishu_operator_sender_id, status, created_at, updated_at, last_message_ts,
                   closed_at
            FROM feishu_session_group_chats
            WHERE uid = ?
              AND session_id = ?
              AND status = 'active'
              AND closed_at <= 0
              AND TRIM(COALESCE(feishu_chat_id, '')) <> ''
            ORDER BY updated_at DESC, id DESC
            "#,
        )
        .bind(uid.trim())
        .bind(session_id.trim())
        .fetch_all(&self.notifications)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| feishu_session_group_chat_from_row(&row))
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_feishu_session_group_chat(
        &self,
        channel_id: i64,
        owner_username: &str,
        uid: &str,
        session_id: &str,
        peer_uid: &str,
        customer_name: &str,
        item_id: &str,
        item_title: &str,
        item_pic: &str,
        item_price: &str,
        feishu_chat_id: &str,
        feishu_chat_name: &str,
        feishu_operator_sender_id: &str,
        status: &str,
        last_message_ts: f64,
    ) -> Result<FeishuSessionGroupChat, StorageError> {
        sqlx::query(
            r#"
            INSERT INTO feishu_session_group_chats (
                channel_id,
                owner_username,
                uid,
                session_id,
                peer_uid,
                customer_name,
                item_id,
                item_title,
                item_pic,
                item_price,
                feishu_chat_id,
                feishu_chat_name,
                feishu_operator_sender_id,
                status,
                created_at,
                updated_at,
                last_message_ts,
                closed_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'), unixepoch('subsec'), ?, 0)
            ON CONFLICT(channel_id, session_id) DO UPDATE SET
                owner_username = excluded.owner_username,
                uid = excluded.uid,
                peer_uid = excluded.peer_uid,
                customer_name = excluded.customer_name,
                item_id = excluded.item_id,
                item_title = excluded.item_title,
                item_pic = excluded.item_pic,
                item_price = excluded.item_price,
                feishu_chat_id = excluded.feishu_chat_id,
                feishu_chat_name = excluded.feishu_chat_name,
                feishu_operator_sender_id = excluded.feishu_operator_sender_id,
                status = CASE
                    WHEN feishu_session_group_chats.closed_at > 0
                      OR feishu_session_group_chats.status IN ('closed', 'won', 'failed')
                    THEN feishu_session_group_chats.status
                    ELSE excluded.status
                END,
                updated_at = excluded.updated_at,
                last_message_ts = excluded.last_message_ts,
                closed_at = feishu_session_group_chats.closed_at
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .bind(uid.trim())
        .bind(session_id.trim())
        .bind(peer_uid.trim())
        .bind(customer_name.trim())
        .bind(item_id.trim())
        .bind(item_title.trim())
        .bind(item_pic.trim())
        .bind(item_price.trim())
        .bind(feishu_chat_id.trim())
        .bind(feishu_chat_name.trim())
        .bind(feishu_operator_sender_id.trim())
        .bind(status.trim())
        .bind(last_message_ts)
        .execute(&self.notifications)
        .await?;

        self.get_feishu_session_group_chat_by_session(channel_id, owner_username, uid, session_id)
            .await?
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::other(
                    "upserted Feishu session group chat missing",
                ))
            })
    }

    pub async fn update_feishu_session_group_status(
        &self,
        channel_id: i64,
        session_id: &str,
        status: &str,
        feishu_chat_name: &str,
        closed_at: Option<f64>,
    ) -> Result<Option<FeishuSessionGroupChat>, StorageError> {
        let row = sqlx::query(
            r#"
            UPDATE feishu_session_group_chats
            SET status = ?,
                feishu_chat_name = ?,
                closed_at = COALESCE(?, closed_at),
                updated_at = unixepoch('subsec')
            WHERE channel_id = ? AND session_id = ?
            RETURNING id, channel_id, owner_username, uid, session_id, peer_uid, customer_name,
                      item_id, item_title, item_pic, item_price, feishu_chat_id, feishu_chat_name,
                      feishu_operator_sender_id, status, created_at, updated_at, last_message_ts,
                      closed_at
            "#,
        )
        .bind(status.trim())
        .bind(feishu_chat_name.trim())
        .bind(closed_at)
        .bind(channel_id)
        .bind(session_id.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_session_group_chat_from_row(&row)))
    }

    pub async fn upsert_feishu_operator_activity(
        &self,
        channel_id: i64,
        owner_username: &str,
        sender_id: &str,
        sender_name: &str,
        last_chat_id: &str,
        last_event_id: &str,
    ) -> Result<FeishuOperatorActivity, StorageError> {
        sqlx::query(
            r#"
            INSERT INTO feishu_operator_activity (
                channel_id,
                owner_username,
                sender_id,
                sender_name,
                last_chat_id,
                last_event_id,
                last_active_at,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, unixepoch('subsec'), unixepoch('subsec'), unixepoch('subsec'))
            ON CONFLICT(channel_id, owner_username, sender_id) DO UPDATE SET
                sender_name = excluded.sender_name,
                last_chat_id = excluded.last_chat_id,
                last_event_id = excluded.last_event_id,
                last_active_at = excluded.last_active_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .bind(sender_id.trim())
        .bind(sender_name.trim())
        .bind(last_chat_id.trim())
        .bind(last_event_id.trim())
        .execute(&self.notifications)
        .await?;

        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, sender_id, sender_name, last_chat_id,
                   last_event_id, last_active_at, created_at, updated_at
            FROM feishu_operator_activity
            WHERE channel_id = ? AND owner_username = ? AND sender_id = ?
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .bind(sender_id.trim())
        .fetch_one(&self.notifications)
        .await?;

        Ok(feishu_operator_activity_from_row(&row))
    }

    pub async fn get_latest_feishu_operator_activity(
        &self,
        channel_id: i64,
        owner_username: &str,
    ) -> Result<Option<FeishuOperatorActivity>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, sender_id, sender_name, last_chat_id,
                   last_event_id, last_active_at, created_at, updated_at
            FROM feishu_operator_activity
            WHERE channel_id = ? AND owner_username = ?
            ORDER BY last_active_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_operator_activity_from_row(&row)))
    }

    pub async fn notification_owner_for_uid(&self, uid: &str) -> Result<String, StorageError> {
        let owner = sqlx::query_scalar::<_, String>(
            "SELECT owner_username FROM accounts WHERE uid = ? LIMIT 1",
        )
        .bind(uid)
        .fetch_optional(&self.accounts)
        .await?;
        Ok(owner.unwrap_or_default())
    }

    pub async fn create_feishu_pending_action(
        &self,
        channel_id: i64,
        owner_username: &str,
        sender_id: &str,
        chat_id: &str,
        nonce: &str,
        action_type: &str,
        uid: &str,
        target_id: &str,
        args: &Map<String, serde_json::Value>,
        source_message_id: &str,
    ) -> Result<FeishuPendingAction, StorageError> {
        let args_json = serde_json::to_string(args)
            .map_err(|error| StorageError::Io(std::io::Error::other(error)))?;
        sqlx::query(
            r#"
            INSERT INTO feishu_pending_actions (
                channel_id,
                owner_username,
                sender_id,
                chat_id,
                nonce,
                action_type,
                uid,
                target_id,
                args_json,
                status,
                source_message_id,
                created_at,
                expires_at,
                confirmed_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, unixepoch('subsec'), unixepoch('subsec') + 300, 0)
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .bind(sender_id.trim())
        .bind(chat_id.trim())
        .bind(nonce.trim())
        .bind(action_type.trim())
        .bind(uid.trim())
        .bind(target_id.trim())
        .bind(args_json)
        .bind(source_message_id.trim())
        .execute(&self.notifications)
        .await?;

        self.get_feishu_pending_action_by_nonce(channel_id, nonce)
            .await?
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::other(
                    "created Feishu pending action missing",
                ))
            })
    }

    pub async fn get_feishu_pending_action_by_nonce(
        &self,
        channel_id: i64,
        nonce: &str,
    ) -> Result<Option<FeishuPendingAction>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, channel_id, owner_username, sender_id, chat_id, nonce,
                   action_type, uid, target_id, args_json, status, source_message_id,
                   created_at, expires_at, confirmed_at
            FROM feishu_pending_actions
            WHERE channel_id = ? AND nonce = ?
            LIMIT 1
            "#,
        )
        .bind(channel_id)
        .bind(nonce.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_pending_action_from_row(&row)))
    }

    pub async fn consume_feishu_pending_action(
        &self,
        channel_id: i64,
        owner_username: &str,
        sender_id: &str,
        chat_id: &str,
        nonce: &str,
    ) -> Result<Option<FeishuPendingAction>, StorageError> {
        let row = sqlx::query(
            r#"
            UPDATE feishu_pending_actions
            SET status = 'confirmed',
                confirmed_at = unixepoch('subsec')
            WHERE id = (
                SELECT id
                FROM feishu_pending_actions
                WHERE channel_id = ?
                  AND owner_username = ?
                  AND sender_id = ?
                  AND chat_id = ?
                  AND nonce = ?
                  AND status = 'pending'
                  AND (expires_at <= 0 OR expires_at > unixepoch('subsec'))
                ORDER BY created_at DESC, id DESC
                LIMIT 1
            )
            RETURNING id, channel_id, owner_username, sender_id, chat_id, nonce,
                      action_type, uid, target_id, args_json, status, source_message_id,
                      created_at, expires_at, confirmed_at
            "#,
        )
        .bind(channel_id)
        .bind(owner_username.trim())
        .bind(sender_id.trim())
        .bind(chat_id.trim())
        .bind(nonce.trim())
        .fetch_optional(&self.notifications)
        .await?;

        Ok(row.map(|row| feishu_pending_action_from_row(&row)))
    }

    pub async fn try_claim_feishu_event(
        &self,
        channel_id: i64,
        event_id: &str,
        message_id: &str,
        chat_id: &str,
        sender_id: &str,
    ) -> Result<bool, StorageError> {
        let Some(receipt_key) = feishu_event_receipt_key(event_id, message_id) else {
            return Ok(true);
        };
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO feishu_event_receipts (
                channel_id,
                event_id,
                message_id,
                chat_id,
                sender_id,
                status,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, 'processing', unixepoch('subsec'), unixepoch('subsec'))
            "#,
        )
        .bind(channel_id)
        .bind(&receipt_key)
        .bind(message_id.trim())
        .bind(chat_id.trim())
        .bind(sender_id.trim())
        .execute(&self.notifications)
        .await?;
        if result.rows_affected() > 0 {
            return Ok(true);
        }

        let result = sqlx::query(
            r#"
            UPDATE feishu_event_receipts
            SET message_id = ?,
                chat_id = ?,
                sender_id = ?,
                status = 'processing',
                updated_at = unixepoch('subsec')
            WHERE channel_id = ?
              AND event_id = ?
              AND (
                  status = 'processing_error'
                  OR (status = 'processing' AND updated_at < unixepoch('subsec') - 120)
              )
            "#,
        )
        .bind(message_id.trim())
        .bind(chat_id.trim())
        .bind(sender_id.trim())
        .bind(channel_id)
        .bind(&receipt_key)
        .execute(&self.notifications)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn finish_feishu_event(
        &self,
        channel_id: i64,
        event_id: &str,
        message_id: &str,
        status: &str,
    ) -> Result<(), StorageError> {
        let Some(receipt_key) = feishu_event_receipt_key(event_id, message_id) else {
            return Ok(());
        };
        sqlx::query(
            r#"
            UPDATE feishu_event_receipts
            SET status = ?, updated_at = unixepoch('subsec')
            WHERE channel_id = ? AND event_id = ?
            "#,
        )
        .bind(status.trim())
        .bind(channel_id)
        .bind(receipt_key)
        .execute(&self.notifications)
        .await?;
        Ok(())
    }
}

fn feishu_pending_action_from_row(row: &SqliteRow) -> FeishuPendingAction {
    FeishuPendingAction {
        id: row.get::<i64, _>("id"),
        channel_id: row.get::<i64, _>("channel_id"),
        owner_username: row.get::<String, _>("owner_username"),
        sender_id: row.get::<String, _>("sender_id"),
        chat_id: row.get::<String, _>("chat_id"),
        nonce: row.get::<String, _>("nonce"),
        action_type: row.get::<String, _>("action_type"),
        uid: row.get::<String, _>("uid"),
        target_id: row.get::<String, _>("target_id"),
        args: parse_json_map(row.get::<String, _>("args_json").as_str()),
        status: row.get::<String, _>("status"),
        source_message_id: row.get::<String, _>("source_message_id"),
        created_at: row.get::<f64, _>("created_at"),
        expires_at: row.get::<f64, _>("expires_at"),
        confirmed_at: row.get::<f64, _>("confirmed_at"),
    }
}

fn feishu_im_reply_route_from_row(row: &SqliteRow) -> FeishuImReplyRoute {
    FeishuImReplyRoute {
        id: row.get::<i64, _>("id"),
        channel_id: row.get::<i64, _>("channel_id"),
        owner_username: row.get::<String, _>("owner_username"),
        feishu_message_id: row.get::<String, _>("feishu_message_id"),
        chat_id: row.get::<String, _>("chat_id"),
        uid: row.get::<String, _>("uid"),
        session_id: row.get::<String, _>("session_id"),
        peer_uid: row.get::<String, _>("peer_uid"),
        customer_name: row.get::<String, _>("customer_name"),
        item_id: row.get::<String, _>("item_id"),
        item_title: row.get::<String, _>("item_title"),
        xianyu_message_id: row.get::<String, _>("xianyu_message_id"),
        xianyu_upstream_message_id: row.get::<String, _>("xianyu_upstream_message_id"),
        created_at: row.get::<f64, _>("created_at"),
        expires_at: row.get::<f64, _>("expires_at"),
    }
}

fn feishu_session_group_chat_from_row(row: &SqliteRow) -> FeishuSessionGroupChat {
    FeishuSessionGroupChat {
        id: row.get::<i64, _>("id"),
        channel_id: row.get::<i64, _>("channel_id"),
        owner_username: row.get::<String, _>("owner_username"),
        uid: row.get::<String, _>("uid"),
        session_id: row.get::<String, _>("session_id"),
        peer_uid: row.get::<String, _>("peer_uid"),
        customer_name: row.get::<String, _>("customer_name"),
        item_id: row.get::<String, _>("item_id"),
        item_title: row.get::<String, _>("item_title"),
        item_pic: row.get::<String, _>("item_pic"),
        item_price: row.get::<String, _>("item_price"),
        feishu_chat_id: row.get::<String, _>("feishu_chat_id"),
        feishu_chat_name: row.get::<String, _>("feishu_chat_name"),
        feishu_operator_sender_id: row.get::<String, _>("feishu_operator_sender_id"),
        status: row.get::<String, _>("status"),
        created_at: row.get::<f64, _>("created_at"),
        updated_at: row.get::<f64, _>("updated_at"),
        last_message_ts: row.get::<f64, _>("last_message_ts"),
        closed_at: row.get::<f64, _>("closed_at"),
    }
}

fn feishu_operator_activity_from_row(row: &SqliteRow) -> FeishuOperatorActivity {
    FeishuOperatorActivity {
        id: row.get::<i64, _>("id"),
        channel_id: row.get::<i64, _>("channel_id"),
        owner_username: row.get::<String, _>("owner_username"),
        sender_id: row.get::<String, _>("sender_id"),
        sender_name: row.get::<String, _>("sender_name"),
        last_chat_id: row.get::<String, _>("last_chat_id"),
        last_event_id: row.get::<String, _>("last_event_id"),
        last_active_at: row.get::<f64, _>("last_active_at"),
        created_at: row.get::<f64, _>("created_at"),
        updated_at: row.get::<f64, _>("updated_at"),
    }
}

fn feishu_user_binding_from_row(row: &SqliteRow) -> FeishuUserBinding {
    FeishuUserBinding {
        id: row.get::<i64, _>("id"),
        channel_id: row.get::<i64, _>("channel_id"),
        owner_username: row.get::<String, _>("owner_username"),
        sender_id: row.get::<String, _>("sender_id"),
        sender_name: row.get::<String, _>("sender_name"),
        chat_id: row.get::<String, _>("chat_id"),
        chat_type: row.get::<String, _>("chat_type"),
        source: row.get::<String, _>("source"),
        role: row.get::<String, _>("role"),
        joined_via: row.get::<String, _>("joined_via"),
        invited_by_sender_id: row.get::<String, _>("invited_by_sender_id"),
        invited_by_sender_name: row.get::<String, _>("invited_by_sender_name"),
        last_message_id: row.get::<String, _>("last_message_id"),
        last_event_id: row.get::<String, _>("last_event_id"),
        last_seen_at: row.get::<f64, _>("last_seen_at"),
        enabled: row.get::<i64, _>("enabled") != 0,
        created_at: row.get::<f64, _>("created_at"),
        updated_at: row.get::<f64, _>("updated_at"),
    }
}

fn feishu_collaborator_invite_from_row(row: &SqliteRow) -> FeishuCollaboratorInvite {
    FeishuCollaboratorInvite {
        id: row.get::<i64, _>("id"),
        channel_id: row.get::<i64, _>("channel_id"),
        owner_username: row.get::<String, _>("owner_username"),
        code: row.get::<String, _>("code"),
        created_by_sender_id: row.get::<String, _>("created_by_sender_id"),
        created_by_sender_name: row.get::<String, _>("created_by_sender_name"),
        status: row.get::<String, _>("status"),
        expires_at: row.get::<f64, _>("expires_at"),
        used_by_sender_id: row.get::<String, _>("used_by_sender_id"),
        used_at: row.get::<f64, _>("used_at"),
        created_at: row.get::<f64, _>("created_at"),
        updated_at: row.get::<f64, _>("updated_at"),
    }
}

fn normalize_notification_config_keys(
    channel_type: &str,
    config: &Map<String, Value>,
) -> Map<String, Value> {
    let mut normalized = config.clone();
    let legacy_robot_url = normalized.remove("webhook_url");

    if matches!(channel_type, "wechat" | "dingtalk") && !normalized.contains_key("robot_url") {
        if let Some(value) = legacy_robot_url {
            normalized.insert("robot_url".to_string(), value);
        }
    }

    normalized
}

fn feishu_event_receipt_key(event_id: &str, message_id: &str) -> Option<String> {
    let event_id = event_id.trim();
    if !event_id.is_empty() {
        return Some(event_id.to_string());
    }
    let message_id = message_id.trim();
    if !message_id.is_empty() {
        return Some(format!("message:{message_id}"));
    }
    None
}

fn filter_bind_all_channels(channels: Vec<NotificationChannel>) -> Vec<NotificationChannel> {
    channels
        .into_iter()
        .filter(|channel| {
            channel.enabled
                && !channel.channel_type.is_empty()
                && channel
                    .config
                    .get("bind_all_accounts")
                    .map(|value| match value {
                        serde_json::Value::Bool(b) => *b,
                        serde_json::Value::Number(n) => n.as_i64() == Some(1),
                        serde_json::Value::String(s) => {
                            let lower = s.trim().to_ascii_lowercase();
                            lower == "true" || lower == "1"
                        }
                        _ => false,
                    })
                    .unwrap_or(false)
        })
        .collect()
}
