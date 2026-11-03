use std::collections::HashSet;

use sqlx::{sqlite::SqliteRow, QueryBuilder, Row, Sqlite};

use crate::models::business::{
    normalize_item_reply_count, AiConversation, AiSettings, AiSettingsUpdatePayload, BusinessOrder,
    Card, CardCreateItem, DeliveryRule, DeliveryRuleCreatePayload, DeliveryRuleUpdatePayload,
    ItemReply, ItemReplyCreatePayload, ItemReplyMode, ItemReplyModeUpsertPayload,
    ItemReplyUpdatePayload, Keyword, KeywordCreatePayload, KeywordUpdatePayload, ReplyMode,
};
use crate::models::runtime::{CacheSyncRuntimeStatus, DeleteRuntimeStatus};

use super::{
    begin_immediate_with_retry, parse_json_map, parse_json_vec_i64, parse_json_vec_strings,
    unix_millis_now, StorageError, StorageState,
};

pub(crate) const GLOBAL_AI_SETTINGS_KEY: &str = "__global__";
pub(crate) const USER_GLOBAL_SCOPE_PREFIX: &str = "__global__::user::";

pub(crate) fn user_global_scope_key(owner_username: &str) -> String {
    let owner = owner_username.trim();
    if owner.is_empty() || owner.eq_ignore_ascii_case("local-app") {
        GLOBAL_AI_SETTINGS_KEY.to_string()
    } else {
        format!("{USER_GLOBAL_SCOPE_PREFIX}{owner}")
    }
}

pub(crate) fn is_global_scope_key(cookie_id: &str) -> bool {
    cookie_id == GLOBAL_AI_SETTINGS_KEY || cookie_id.starts_with(USER_GLOBAL_SCOPE_PREFIX)
}

pub(crate) fn public_cookie_id(cookie_id: &str) -> String {
    if is_global_scope_key(cookie_id) {
        GLOBAL_AI_SETTINGS_KEY.to_string()
    } else {
        cookie_id.to_string()
    }
}

impl StorageState {
    async fn global_scope_key_for_account(&self, cookie_id: &str) -> Result<String, StorageError> {
        if is_global_scope_key(cookie_id) {
            return Ok(cookie_id.to_string());
        }

        let owner = sqlx::query_scalar::<_, String>(
            "SELECT owner_username FROM accounts WHERE uid = ? LIMIT 1",
        )
        .bind(cookie_id.trim())
        .fetch_optional(&self.accounts)
        .await?
        .unwrap_or_default();

        Ok(user_global_scope_key(&owner))
    }

    pub async fn list_keywords(&self, cookie_id: &str) -> Result<Vec<Keyword>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, cookie_id, keyword, reply, type, item_id, enabled, task_group_id, created_at
            FROM keywords
            WHERE cookie_id = ?
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .bind(cookie_id)
        .fetch_all(&self.accounts)
        .await?;

        Ok(rows
            .into_iter()
            .map(keyword_from_row)
            .map(public_keyword)
            .collect())
    }

    pub async fn list_keywords_with_global(
        &self,
        cookie_id: &str,
    ) -> Result<Vec<Keyword>, StorageError> {
        if is_global_scope_key(cookie_id) {
            return self.list_keywords(cookie_id).await;
        }

        let rows = sqlx::query(
            r#"
            SELECT id, cookie_id, keyword, reply, type, item_id, enabled, task_group_id, created_at
            FROM keywords
            WHERE cookie_id = ?
            ORDER BY
                created_at DESC,
                id DESC
            "#,
        )
        .bind(cookie_id)
        .fetch_all(&self.accounts)
        .await?;

        Ok(rows
            .into_iter()
            .map(keyword_from_row)
            .map(public_keyword)
            .collect())
    }

    pub async fn create_keyword(
        &self,
        cookie_id: &str,
        payload: &KeywordCreatePayload,
    ) -> Result<Keyword, StorageError> {
        let cursor = sqlx::query(
            r#"
            INSERT INTO keywords (cookie_id, keyword, reply, type, item_id, enabled, task_group_id, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(cookie_id)
        .bind(&payload.keyword)
        .bind(&payload.reply)
        .bind(default_kind(payload.kind.as_str()))
        .bind(&payload.item_id)
        .bind(if payload.enabled { 1_i64 } else { 0_i64 })
        .bind(payload.task_group_id.trim())
        .bind(unix_millis_now())
        .execute(&self.accounts)
        .await?;

        self.get_keyword(cursor.last_insert_rowid())
            .await?
            .map(public_keyword)
            .ok_or_else(|| StorageError::Io(std::io::Error::other("created keyword missing")))
    }

    pub async fn update_keyword(
        &self,
        keyword_id: i64,
        payload: &KeywordUpdatePayload,
    ) -> Result<Option<Keyword>, StorageError> {
        if self.get_keyword(keyword_id).await?.is_none() {
            return Ok(None);
        }

        let mut builder = QueryBuilder::<Sqlite>::new("UPDATE keywords SET ");
        let mut changed = false;
        let mut first = true;

        let mut push_assignment = |builder: &mut QueryBuilder<Sqlite>, column: &str| {
            if !first {
                builder.push(", ");
            }
            first = false;
            builder.push(column).push(" = ");
        };

        if let Some(cookie_id) = payload.cookie_id.as_ref() {
            push_assignment(&mut builder, "cookie_id");
            builder.push_bind(cookie_id);
            changed = true;
        }
        if let Some(keyword) = payload.keyword.as_ref() {
            push_assignment(&mut builder, "keyword");
            builder.push_bind(keyword);
            changed = true;
        }
        if let Some(reply) = payload.reply.as_ref() {
            push_assignment(&mut builder, "reply");
            builder.push_bind(reply);
            changed = true;
        }
        if let Some(kind) = payload.kind.as_ref() {
            push_assignment(&mut builder, "type");
            builder.push_bind(default_kind(kind.as_str()));
            changed = true;
        }
        if let Some(item_id) = payload.item_id.as_ref() {
            push_assignment(&mut builder, "item_id");
            builder.push_bind(item_id);
            changed = true;
        }
        if let Some(enabled) = payload.enabled {
            push_assignment(&mut builder, "enabled");
            builder.push_bind(if enabled { 1_i64 } else { 0_i64 });
            changed = true;
        }
        if let Some(task_group_id) = payload.task_group_id.as_ref() {
            push_assignment(&mut builder, "task_group_id");
            builder.push_bind(task_group_id.trim());
            changed = true;
        }

        if !changed {
            return Ok(self.get_keyword(keyword_id).await?.map(public_keyword));
        }

        builder.push(" WHERE id = ").push_bind(keyword_id);
        builder.build().execute(&self.accounts).await?;

        Ok(self.get_keyword(keyword_id).await?.map(public_keyword))
    }

    pub async fn delete_keyword(&self, keyword_id: i64) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM keywords WHERE id = ?")
            .bind(keyword_id)
            .execute(&self.accounts)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_item_replies(&self, cookie_id: &str) -> Result<Vec<ItemReply>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                cookie_id,
                item_id,
                reply,
                type,
                enabled,
                auto_recall,
                task_group_id,
                reply_count,
                created_at
            FROM item_replies
            WHERE cookie_id = ?
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .bind(cookie_id)
        .fetch_all(&self.accounts)
        .await?;

        Ok(rows
            .into_iter()
            .map(item_reply_from_row)
            .map(public_item_reply)
            .collect())
    }

    pub async fn create_item_reply(
        &self,
        cookie_id: &str,
        payload: &ItemReplyCreatePayload,
    ) -> Result<ItemReply, StorageError> {
        let cursor = sqlx::query(
            r#"
            INSERT INTO item_replies (
                cookie_id,
                item_id,
                reply,
                type,
                enabled,
                auto_recall,
                task_group_id,
                reply_count,
                created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(cookie_id)
        .bind(&payload.item_id)
        .bind(&payload.reply)
        .bind(default_kind(payload.kind.as_str()))
        .bind(if payload.enabled { 1_i64 } else { 0_i64 })
        .bind(if payload.auto_recall { 1_i64 } else { 0_i64 })
        .bind(payload.task_group_id.trim())
        .bind(normalize_item_reply_count(payload.reply_count))
        .bind(unix_millis_now())
        .execute(&self.accounts)
        .await?;

        self.get_item_reply(cursor.last_insert_rowid())
            .await?
            .map(public_item_reply)
            .ok_or_else(|| StorageError::Io(std::io::Error::other("created item reply missing")))
    }

    pub async fn update_item_reply(
        &self,
        item_reply_id: i64,
        payload: &ItemReplyUpdatePayload,
    ) -> Result<Option<ItemReply>, StorageError> {
        if self.get_item_reply(item_reply_id).await?.is_none() {
            return Ok(None);
        }

        let mut builder = QueryBuilder::<Sqlite>::new("UPDATE item_replies SET ");
        let mut changed = false;
        let mut first = true;

        let mut push_assignment = |builder: &mut QueryBuilder<Sqlite>, column: &str| {
            if !first {
                builder.push(", ");
            }
            first = false;
            builder.push(column).push(" = ");
        };

        if let Some(cookie_id) = payload.cookie_id.as_ref() {
            push_assignment(&mut builder, "cookie_id");
            builder.push_bind(cookie_id);
            changed = true;
        }
        if let Some(item_id) = payload.item_id.as_ref() {
            push_assignment(&mut builder, "item_id");
            builder.push_bind(item_id);
            changed = true;
        }
        if let Some(reply) = payload.reply.as_ref() {
            push_assignment(&mut builder, "reply");
            builder.push_bind(reply);
            changed = true;
        }
        if let Some(kind) = payload.kind.as_ref() {
            push_assignment(&mut builder, "type");
            builder.push_bind(default_kind(kind.as_str()));
            changed = true;
        }
        if let Some(enabled) = payload.enabled {
            push_assignment(&mut builder, "enabled");
            builder.push_bind(if enabled { 1_i64 } else { 0_i64 });
            changed = true;
        }
        if let Some(auto_recall) = payload.auto_recall {
            push_assignment(&mut builder, "auto_recall");
            builder.push_bind(if auto_recall { 1_i64 } else { 0_i64 });
            changed = true;
        }
        if let Some(task_group_id) = payload.task_group_id.as_ref() {
            push_assignment(&mut builder, "task_group_id");
            builder.push_bind(task_group_id.trim());
            changed = true;
        }
        if let Some(reply_count) = payload.reply_count {
            push_assignment(&mut builder, "reply_count");
            builder.push_bind(normalize_item_reply_count(reply_count));
            changed = true;
        }

        if !changed {
            return Ok(self
                .get_item_reply(item_reply_id)
                .await?
                .map(public_item_reply));
        }

        builder.push(" WHERE id = ").push_bind(item_reply_id);
        builder.build().execute(&self.accounts).await?;

        Ok(self
            .get_item_reply(item_reply_id)
            .await?
            .map(public_item_reply))
    }

    pub async fn delete_item_reply(&self, item_reply_id: i64) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM item_replies WHERE id = ?")
            .bind(item_reply_id)
            .execute(&self.accounts)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_item_reply_modes(
        &self,
        cookie_id: &str,
    ) -> Result<Vec<ItemReplyMode>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT cookie_id, item_id, reply_mode, updated_at
            FROM item_reply_modes
            WHERE cookie_id = ?
            ORDER BY updated_at DESC, item_id ASC
            "#,
        )
        .bind(cookie_id)
        .fetch_all(&self.accounts)
        .await?;

        Ok(rows
            .into_iter()
            .map(item_reply_mode_from_row)
            .map(public_item_reply_mode)
            .collect())
    }

    pub async fn get_item_reply_mode(
        &self,
        cookie_id: &str,
        item_id: &str,
    ) -> Result<Option<ItemReplyMode>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT cookie_id, item_id, reply_mode, updated_at
            FROM item_reply_modes
            WHERE cookie_id = ? AND item_id = ?
            LIMIT 1
            "#,
        )
        .bind(cookie_id)
        .bind(item_id)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(row.map(item_reply_mode_from_row))
    }

    pub async fn upsert_item_reply_mode(
        &self,
        cookie_id: &str,
        payload: &ItemReplyModeUpsertPayload,
    ) -> Result<ItemReplyMode, StorageError> {
        let now = unix_millis_now();
        let item_id = payload.item_id.trim();
        let reply_mode = payload.reply_mode.normalize_item();

        sqlx::query(
            r#"
            INSERT INTO item_reply_modes (
                cookie_id,
                item_id,
                reply_mode,
                updated_at
            )
            VALUES (?, ?, ?, ?)
            ON CONFLICT(cookie_id, item_id) DO UPDATE SET
                reply_mode = excluded.reply_mode,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(cookie_id)
        .bind(item_id)
        .bind(reply_mode.as_str())
        .bind(now)
        .execute(&self.accounts)
        .await?;

        self.get_item_reply_mode(cookie_id, item_id)
            .await?
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::other("upserted item reply mode missing"))
            })
    }

    pub async fn delete_item_reply_mode(
        &self,
        cookie_id: &str,
        item_id: &str,
    ) -> Result<bool, StorageError> {
        let result =
            sqlx::query("DELETE FROM item_reply_modes WHERE cookie_id = ? AND item_id = ?")
                .bind(cookie_id)
                .bind(item_id)
                .execute(&self.accounts)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_cards(&self, cookie_id: &str) -> Result<Vec<Card>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, cookie_id, card_key, card_content, card_pool_tag, reusable, used,
                   one_time_identity_active, used_at, used_order_id, created_at
            FROM cards
            WHERE cookie_id = ?
            ORDER BY used ASC, created_at DESC, id DESC
            "#,
        )
        .bind(cookie_id)
        .fetch_all(&self.accounts)
        .await?;

        Ok(rows
            .into_iter()
            .map(card_from_row)
            .map(public_card)
            .collect())
    }

    pub async fn bulk_create_cards(
        &self,
        cookie_id: &str,
        cards: &[CardCreateItem],
    ) -> Result<Vec<Card>, StorageError> {
        let mut normalized_cards = Vec::with_capacity(cards.len());
        let mut request_seen = HashSet::new();

        for card in cards {
            let pool_tag = card.card_pool_tag.trim().to_string();
            let normalized_content = card.card_content.trim().to_string();
            if !card.reusable
                && !normalized_content.is_empty()
                && !request_seen.insert((pool_tag.clone(), normalized_content))
            {
                return Err(StorageError::DuplicateOneTimeCard {
                    owner_scope_key: cookie_id.to_string(),
                    pool_tag,
                });
            }
            normalized_cards.push(NormalizedCardWrite {
                card_key: card.card_key.clone(),
                card_content: card.card_content.clone(),
                card_pool_tag: pool_tag,
                reusable: card.reusable,
            });
        }

        let mut tx = begin_immediate_with_retry(&self.accounts, "bulk_create_cards").await?;
        let mut created_ids = Vec::with_capacity(normalized_cards.len());
        for card in &normalized_cards {
            let result = sqlx::query(
                r#"
                INSERT INTO cards (cookie_id, card_key, card_content, card_pool_tag, reusable, used, used_at, used_order_id, created_at)
                VALUES (?, ?, ?, ?, ?, 0, 0, '', ?)
                "#,
            )
            .bind(cookie_id)
            .bind(&card.card_key)
            .bind(&card.card_content)
            .bind(&card.card_pool_tag)
            .bind(if card.reusable { 1_i64 } else { 0_i64 })
            .bind(unix_millis_now())
            .execute(&mut *tx)
            .await
            .map_err(|error| map_card_write_error(error, cookie_id, &card.card_pool_tag))?;
            if result.rows_affected() == 0 {
                return Err(StorageError::DuplicateOneTimeCard {
                    owner_scope_key: cookie_id.to_string(),
                    pool_tag: card.card_pool_tag.clone(),
                });
            }
            created_ids.push(result.last_insert_rowid());
        }
        tx.commit().await?;

        let mut created = Vec::with_capacity(created_ids.len());
        for card_id in created_ids {
            if let Some(card) = self.get_card(card_id).await? {
                created.push(public_card(card));
            }
        }
        Ok(created)
    }

    pub async fn card_pool_exists(
        &self,
        cookie_id: &str,
        card_pool_tag: &str,
    ) -> Result<bool, StorageError> {
        self.card_pool_exists_scoped(cookie_id, "account", card_pool_tag)
            .await
    }

    pub async fn update_card(
        &self,
        card_id: i64,
        card_content: &str,
        card_pool_tag: &str,
    ) -> Result<Option<Card>, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "update_card").await?;
        let current = sqlx::query(
            "SELECT cookie_id, reusable, one_time_identity_active FROM cards WHERE id = ? LIMIT 1",
        )
        .bind(card_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(current) = current else {
            tx.commit().await?;
            return Ok(None);
        };
        let cookie_id = current.get::<String, _>("cookie_id");
        let reusable = current.get::<i64, _>("reusable") != 0;
        let one_time_identity_active = current.get::<i64, _>("one_time_identity_active") != 0;
        if !reusable && !one_time_identity_active {
            tx.rollback().await?;
            return Err(StorageError::Conflict {
                message: "隔离卡密不允许编辑，请删除后重新导入".to_string(),
            });
        }
        let next_pool_tag = card_pool_tag.trim().to_string();

        sqlx::query("UPDATE cards SET card_content = ?, card_pool_tag = ? WHERE id = ?")
            .bind(card_content)
            .bind(&next_pool_tag)
            .bind(card_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| map_card_write_error(error, &cookie_id, &next_pool_tag))?;
        tx.commit().await?;

        let card = self.get_card(card_id).await?.map(public_card);
        if reusable {
            return Ok(card);
        }
        Ok(card)
    }

    pub async fn card_pool_exists_scoped(
        &self,
        cookie_id: &str,
        pool_scope: &str,
        card_pool_tag: &str,
    ) -> Result<bool, StorageError> {
        let resolved_cookie_id = self
            .resolve_card_pool_cookie_id(cookie_id, pool_scope)
            .await?;
        let row = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM cards
            WHERE cookie_id = ?
              AND card_pool_tag = ?
              AND TRIM(card_content) <> ''
              AND (reusable = 1 OR one_time_identity_active = 1)
            LIMIT 1
            "#,
        )
        .bind(resolved_cookie_id)
        .bind(card_pool_tag)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(row.is_some())
    }

    pub async fn delete_card(&self, card_id: i64) -> Result<bool, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "delete_card").await?;
        let row = sqlx::query(
            "SELECT reserved_run_id, reusable, one_time_identity_active FROM cards WHERE id = ?",
        )
        .bind(card_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(false);
        };
        let reserved_run_id = row.get::<Option<i64>, _>("reserved_run_id");
        let reusable = row.get::<i64, _>("reusable") != 0;
        let one_time_identity_active = row.get::<i64, _>("one_time_identity_active") != 0;
        let effectively_reserved = reserved_run_id
            .is_some_and(|run_id| run_id > 0 || reusable || one_time_identity_active);
        if effectively_reserved {
            let run_id = reserved_run_id.unwrap_or_default();
            return Err(StorageError::Conflict {
                message: format!("card {card_id} is reserved by delivery run {run_id}"),
            });
        }

        let result = sqlx::query(
            r#"
            DELETE FROM cards
            WHERE id = ?
              AND (
                    reserved_run_id IS NULL
                    OR (reusable = 0 AND one_time_identity_active = 0 AND reserved_run_id <= 0)
                  )
            "#,
        )
        .bind(card_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn list_delivery_rules(
        &self,
        cookie_id: &str,
    ) -> Result<Vec<DeliveryRule>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, cookie_id, keyword, item_id, card_pool_tag, card_pool_scope, enabled, created_at
            FROM delivery_rules
            WHERE cookie_id = ?
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .bind(cookie_id)
        .fetch_all(&self.accounts)
        .await?;

        Ok(rows
            .into_iter()
            .map(delivery_rule_from_row)
            .map(public_delivery_rule)
            .collect())
    }

    pub async fn list_delivery_rules_with_global(
        &self,
        cookie_id: &str,
    ) -> Result<Vec<DeliveryRule>, StorageError> {
        let global_cookie_id = self.global_scope_key_for_account(cookie_id).await?;
        let rows = sqlx::query(
            r#"
            SELECT id, cookie_id, keyword, item_id, card_pool_tag, card_pool_scope, enabled, created_at
            FROM delivery_rules
            WHERE cookie_id = ? OR cookie_id = ?
            ORDER BY
                CASE WHEN cookie_id = ? THEN 0 ELSE 1 END ASC,
                created_at DESC,
                id DESC
            "#,
        )
        .bind(cookie_id)
        .bind(global_cookie_id)
        .bind(cookie_id)
        .fetch_all(&self.accounts)
        .await?;

        Ok(rows
            .into_iter()
            .map(delivery_rule_from_row)
            .map(public_delivery_rule)
            .collect())
    }

    pub async fn get_delivery_rule(
        &self,
        rule_id: i64,
    ) -> Result<Option<DeliveryRule>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, cookie_id, keyword, item_id, card_pool_tag, card_pool_scope, enabled, created_at
            FROM delivery_rules
            WHERE id = ?
            "#,
        )
        .bind(rule_id)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(row.map(delivery_rule_from_row))
    }

    pub async fn create_delivery_rule(
        &self,
        cookie_id: &str,
        payload: &DeliveryRuleCreatePayload,
    ) -> Result<DeliveryRule, StorageError> {
        let cursor = sqlx::query(
            r#"
            INSERT INTO delivery_rules (cookie_id, keyword, item_id, card_pool_tag, card_pool_scope, enabled, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(cookie_id)
        .bind(&payload.keyword)
        .bind(&payload.item_id)
        .bind(&payload.card_pool_tag)
        .bind(normalize_card_pool_scope(payload.card_pool_scope.as_str()))
        .bind(if payload.enabled { 1_i64 } else { 0_i64 })
        .bind(unix_millis_now())
        .execute(&self.accounts)
        .await?;

        self.get_delivery_rule(cursor.last_insert_rowid())
            .await?
            .map(public_delivery_rule)
            .ok_or_else(|| StorageError::Io(std::io::Error::other("created delivery rule missing")))
    }

    pub async fn update_delivery_rule(
        &self,
        rule_id: i64,
        payload: &DeliveryRuleUpdatePayload,
    ) -> Result<Option<DeliveryRule>, StorageError> {
        if self.get_delivery_rule(rule_id).await?.is_none() {
            return Ok(None);
        }

        let mut builder = QueryBuilder::<Sqlite>::new("UPDATE delivery_rules SET ");
        let mut changed = false;
        let mut first = true;

        let mut push_assignment = |builder: &mut QueryBuilder<Sqlite>, column: &str| {
            if !first {
                builder.push(", ");
            }
            first = false;
            builder.push(column).push(" = ");
        };

        if let Some(cookie_id) = payload.cookie_id.as_ref() {
            push_assignment(&mut builder, "cookie_id");
            builder.push_bind(cookie_id);
            changed = true;
        }
        if let Some(keyword) = payload.keyword.as_ref() {
            push_assignment(&mut builder, "keyword");
            builder.push_bind(keyword);
            changed = true;
        }
        if let Some(item_id) = payload.item_id.as_ref() {
            push_assignment(&mut builder, "item_id");
            builder.push_bind(item_id);
            changed = true;
        }
        if let Some(card_pool_tag) = payload.card_pool_tag.as_ref() {
            push_assignment(&mut builder, "card_pool_tag");
            builder.push_bind(card_pool_tag);
            changed = true;
        }
        if let Some(card_pool_scope) = payload.card_pool_scope.as_ref() {
            push_assignment(&mut builder, "card_pool_scope");
            builder.push_bind(normalize_card_pool_scope(card_pool_scope.as_str()));
            changed = true;
        }
        if let Some(enabled) = payload.enabled {
            push_assignment(&mut builder, "enabled");
            builder.push_bind(if enabled { 1_i64 } else { 0_i64 });
            changed = true;
        }

        if !changed {
            return Ok(self
                .get_delivery_rule(rule_id)
                .await?
                .map(public_delivery_rule));
        }

        builder.push(" WHERE id = ").push_bind(rule_id);
        builder.build().execute(&self.accounts).await?;

        Ok(self
            .get_delivery_rule(rule_id)
            .await?
            .map(public_delivery_rule))
    }

    pub async fn delete_delivery_rule(&self, rule_id: i64) -> Result<bool, StorageError> {
        let result = sqlx::query("DELETE FROM delivery_rules WHERE id = ?")
            .bind(rule_id)
            .execute(&self.accounts)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_keyword(&self, keyword_id: i64) -> Result<Option<Keyword>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, cookie_id, keyword, reply, type, item_id, enabled, task_group_id, created_at
            FROM keywords
            WHERE id = ?
            "#,
        )
        .bind(keyword_id)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(row.map(keyword_from_row))
    }

    pub async fn get_item_reply(
        &self,
        item_reply_id: i64,
    ) -> Result<Option<ItemReply>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                cookie_id,
                item_id,
                reply,
                type,
                enabled,
                auto_recall,
                task_group_id,
                reply_count,
                created_at
            FROM item_replies
            WHERE id = ?
            "#,
        )
        .bind(item_reply_id)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(row.map(item_reply_from_row))
    }

    pub async fn get_card(&self, card_id: i64) -> Result<Option<Card>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, cookie_id, card_key, card_content, card_pool_tag, reusable, used,
                   one_time_identity_active, used_at, used_order_id, created_at
            FROM cards
            WHERE id = ?
            "#,
        )
        .bind(card_id)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(row.map(card_from_row))
    }

    async fn fetch_ai_settings_row(
        &self,
        cookie_id: &str,
    ) -> Result<Option<SqliteRow>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT
                cookie_id,
                reply_mode,
                ai_enabled,
                auto_reply_enabled,
                auto_ship_enabled,
                api_type,
                api_key,
                base_url,
                model_name,
                max_bargain_rounds,
                max_discount_percent,
                max_discount_amount,
                custom_prompts,
                updated_at
            FROM ai_reply_settings
            WHERE cookie_id = ?
            "#,
        )
        .bind(cookie_id)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(row)
    }

    async fn fetch_platform_messages_enabled(
        &self,
        cookie_id: &str,
    ) -> Result<Option<bool>, StorageError> {
        let value = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT platform_messages_enabled
            FROM ai_reply_settings
            WHERE cookie_id = ?
            LIMIT 1
            "#,
        )
        .bind(cookie_id)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(value.map(|flag| flag != 0))
    }

    pub async fn get_platform_messages_enabled_for_owner(
        &self,
        owner_username: &str,
    ) -> Result<bool, StorageError> {
        let cookie_id = user_global_scope_key(owner_username);
        Ok(self
            .fetch_platform_messages_enabled(&cookie_id)
            .await?
            .unwrap_or(true))
    }

    pub async fn upsert_platform_messages_enabled_for_owner(
        &self,
        owner_username: &str,
        enabled: bool,
    ) -> Result<bool, StorageError> {
        let cookie_id = user_global_scope_key(owner_username);
        let now = unix_millis_now();

        sqlx::query(
            r#"
            INSERT INTO ai_reply_settings (
                cookie_id,
                platform_messages_enabled,
                updated_at
            )
            VALUES (?, ?, ?)
            ON CONFLICT(cookie_id) DO UPDATE SET
                platform_messages_enabled = excluded.platform_messages_enabled,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&cookie_id)
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(now)
        .execute(&self.accounts)
        .await?;

        Ok(enabled)
    }

    pub async fn get_platform_messages_enabled_for_uid(
        &self,
        uid: &str,
    ) -> Result<bool, StorageError> {
        let cookie_id = self.global_scope_key_for_account(uid).await?;
        Ok(self
            .fetch_platform_messages_enabled(&cookie_id)
            .await?
            .unwrap_or(true))
    }

    pub async fn get_ai_settings(
        &self,
        cookie_id: &str,
    ) -> Result<Option<AiSettings>, StorageError> {
        let global_cookie_id = self.global_scope_key_for_account(cookie_id).await?;
        let global_row = self.fetch_ai_settings_row(&global_cookie_id).await?;

        if is_global_scope_key(cookie_id) {
            return Ok(global_row
                .as_ref()
                .map(|row| ai_settings_from_row(row, GLOBAL_AI_SETTINGS_KEY)));
        }

        let account_row = self.fetch_ai_settings_row(cookie_id).await?;
        let config_row = global_row.as_ref();

        let Some(config_row) = config_row else {
            return Ok(account_row.as_ref().map(|row| AiSettings {
                cookie_id: cookie_id.to_string(),
                reply_mode: ai_row_reply_mode(row),
                ai_enabled: ai_row_reply_mode(row).ai_enabled(),
                auto_reply_enabled: ai_row_reply_mode(row).auto_reply_enabled(),
                auto_ship_enabled: ai_row_bool(row, "auto_ship_enabled"),
                updated_at: ai_row_updated_at(row),
                api_type: "openai".to_string(),
                ..AiSettings::default()
            }));
        };

        let account_reply_mode = account_row
            .as_ref()
            .map(ai_row_reply_mode)
            .unwrap_or_default()
            .normalize_account();
        let account_auto_ship_enabled = account_row
            .as_ref()
            .map(|row| ai_row_bool(row, "auto_ship_enabled"))
            .unwrap_or(false);
        let updated_at = account_row
            .as_ref()
            .map(ai_row_updated_at)
            .unwrap_or_else(|| ai_row_updated_at(config_row));

        Ok(Some(AiSettings {
            cookie_id: cookie_id.to_string(),
            reply_mode: account_reply_mode,
            ai_enabled: account_reply_mode.ai_enabled(),
            auto_reply_enabled: account_reply_mode.auto_reply_enabled(),
            auto_ship_enabled: account_auto_ship_enabled,
            api_type: config_row.get::<String, _>("api_type"),
            api_key: config_row.get::<String, _>("api_key"),
            base_url: config_row.get::<String, _>("base_url"),
            model_name: config_row.get::<String, _>("model_name"),
            max_bargain_rounds: config_row.get::<i64, _>("max_bargain_rounds"),
            max_discount_percent: config_row.get::<f64, _>("max_discount_percent"),
            max_discount_amount: config_row.get::<f64, _>("max_discount_amount"),
            custom_prompts: parse_json_map(config_row.get::<String, _>("custom_prompts").as_str()),
            updated_at,
        }))
    }

    pub async fn upsert_ai_settings(
        &self,
        cookie_id: &str,
        payload: &AiSettingsUpdatePayload,
    ) -> Result<AiSettings, StorageError> {
        let now = unix_millis_now();

        let existing_row = self.fetch_ai_settings_row(cookie_id).await?;
        let inherited_global_key = self.global_scope_key_for_account(cookie_id).await?;
        let inherited = self
            .get_ai_settings(&inherited_global_key)
            .await?
            .unwrap_or_else(|| AiSettings {
                cookie_id: GLOBAL_AI_SETTINGS_KEY.to_string(),
                reply_mode: ReplyMode::Off,
                api_type: "openai".to_string(),
                ..AiSettings::default()
            });
        let writes_config = ai_payload_has_config(payload);

        let writes_config = writes_config && is_global_scope_key(cookie_id);
        let sanitized_existing_for_account = if is_global_scope_key(cookie_id) {
            existing_row
                .as_ref()
                .map(|row| ai_settings_from_row(row, cookie_id))
        } else {
            existing_row.as_ref().map(|row| AiSettings {
                cookie_id: cookie_id.to_string(),
                reply_mode: ai_row_reply_mode(row),
                ai_enabled: ai_row_reply_mode(row).ai_enabled(),
                auto_reply_enabled: ai_row_reply_mode(row).auto_reply_enabled(),
                auto_ship_enabled: ai_row_bool(row, "auto_ship_enabled"),
                api_type: "openai".to_string(),
                updated_at: ai_row_updated_at(row),
                ..AiSettings::default()
            })
        };

        let next = if writes_config {
            let preserved_reply_mode = existing_row
                .as_ref()
                .map(ai_row_reply_mode)
                .unwrap_or_default();
            let next_reply_mode = resolve_next_reply_mode(
                cookie_id,
                preserved_reply_mode,
                payload.reply_mode,
                payload.ai_enabled,
                payload.auto_reply_enabled,
            );
            AiSettings {
                cookie_id: cookie_id.to_string(),
                reply_mode: next_reply_mode,
                ai_enabled: next_reply_mode.ai_enabled(),
                auto_reply_enabled: next_reply_mode.auto_reply_enabled(),
                auto_ship_enabled: payload.auto_ship_enabled.unwrap_or_else(|| {
                    existing_row
                        .as_ref()
                        .map(|row| ai_row_bool(row, "auto_ship_enabled"))
                        .unwrap_or(false)
                }),
                api_type: payload
                    .api_type
                    .clone()
                    .unwrap_or_else(|| inherited.api_type.clone()),
                api_key: payload
                    .api_key
                    .clone()
                    .unwrap_or_else(|| inherited.api_key.clone()),
                base_url: payload
                    .base_url
                    .clone()
                    .unwrap_or_else(|| inherited.base_url.clone()),
                model_name: payload
                    .model_name
                    .clone()
                    .unwrap_or_else(|| inherited.model_name.clone()),
                max_bargain_rounds: payload
                    .max_bargain_rounds
                    .unwrap_or(inherited.max_bargain_rounds),
                max_discount_percent: payload
                    .max_discount_percent
                    .unwrap_or(inherited.max_discount_percent),
                max_discount_amount: payload
                    .max_discount_amount
                    .unwrap_or(inherited.max_discount_amount),
                custom_prompts: payload
                    .custom_prompts
                    .clone()
                    .unwrap_or_else(|| inherited.custom_prompts.clone()),
                updated_at: now,
            }
        } else {
            let preserved = sanitized_existing_for_account.unwrap_or_else(|| AiSettings {
                cookie_id: cookie_id.to_string(),
                reply_mode: ReplyMode::Off,
                api_type: "openai".to_string(),
                ..AiSettings::default()
            });
            let next_reply_mode = resolve_next_reply_mode(
                cookie_id,
                preserved.reply_mode,
                payload.reply_mode,
                payload.ai_enabled,
                payload.auto_reply_enabled,
            );
            AiSettings {
                reply_mode: next_reply_mode,
                ai_enabled: next_reply_mode.ai_enabled(),
                auto_reply_enabled: next_reply_mode.auto_reply_enabled(),
                auto_ship_enabled: payload
                    .auto_ship_enabled
                    .unwrap_or(preserved.auto_ship_enabled),
                api_type: if is_global_scope_key(cookie_id) {
                    preserved.api_type
                } else {
                    "openai".to_string()
                },
                api_key: if is_global_scope_key(cookie_id) {
                    preserved.api_key
                } else {
                    String::new()
                },
                base_url: if is_global_scope_key(cookie_id) {
                    preserved.base_url
                } else {
                    String::new()
                },
                model_name: if is_global_scope_key(cookie_id) {
                    preserved.model_name
                } else {
                    String::new()
                },
                max_bargain_rounds: if is_global_scope_key(cookie_id) {
                    preserved.max_bargain_rounds
                } else {
                    0
                },
                max_discount_percent: if is_global_scope_key(cookie_id) {
                    preserved.max_discount_percent
                } else {
                    0.0
                },
                max_discount_amount: if is_global_scope_key(cookie_id) {
                    preserved.max_discount_amount
                } else {
                    0.0
                },
                custom_prompts: if is_global_scope_key(cookie_id) {
                    preserved.custom_prompts
                } else {
                    serde_json::Map::new()
                },
                updated_at: now,
                cookie_id: preserved.cookie_id,
            }
        };

        let serialized_prompts = serde_json::to_string(&next.custom_prompts)
            .map_err(|error| StorageError::Io(std::io::Error::other(error)))?;

        sqlx::query(
            r#"
            INSERT INTO ai_reply_settings (
                cookie_id,
                reply_mode,
                ai_enabled,
                auto_reply_enabled,
                auto_ship_enabled,
                api_type,
                api_key,
                base_url,
                model_name,
                max_bargain_rounds,
                max_discount_percent,
                max_discount_amount,
                custom_prompts,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(cookie_id) DO UPDATE SET
                reply_mode = excluded.reply_mode,
                ai_enabled = excluded.ai_enabled,
                auto_reply_enabled = excluded.auto_reply_enabled,
                auto_ship_enabled = excluded.auto_ship_enabled,
                api_type = excluded.api_type,
                api_key = excluded.api_key,
                base_url = excluded.base_url,
                model_name = excluded.model_name,
                max_bargain_rounds = excluded.max_bargain_rounds,
                max_discount_percent = excluded.max_discount_percent,
                max_discount_amount = excluded.max_discount_amount,
                custom_prompts = excluded.custom_prompts,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(cookie_id)
        .bind(next.reply_mode.as_str())
        .bind(if next.ai_enabled { 1_i64 } else { 0_i64 })
        .bind(if next.auto_reply_enabled {
            1_i64
        } else {
            0_i64
        })
        .bind(if next.auto_ship_enabled { 1_i64 } else { 0_i64 })
        .bind(&next.api_type)
        .bind(&next.api_key)
        .bind(&next.base_url)
        .bind(&next.model_name)
        .bind(next.max_bargain_rounds)
        .bind(next.max_discount_percent)
        .bind(next.max_discount_amount)
        .bind(serialized_prompts)
        .bind(now)
        .execute(&self.accounts)
        .await?;

        self.get_ai_settings(cookie_id)
            .await?
            .ok_or_else(|| StorageError::Io(std::io::Error::other("upserted ai settings missing")))
    }

    pub async fn get_business_order(
        &self,
        cookie_id: &str,
        order_id: &str,
    ) -> Result<Option<BusinessOrder>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT
                order_id,
                cookie_id,
                item_id,
                buyer_id,
                amount,
                status,
                last_shipped_at,
                last_msg_at,
                created_at,
                updated_at
            FROM biz_orders
            WHERE cookie_id = ? AND order_id = ?
            "#,
        )
        .bind(cookie_id)
        .bind(order_id)
        .fetch_optional(&self.accounts)
        .await?;

        Ok(row.map(|row| BusinessOrder {
            order_id: row.get::<String, _>("order_id"),
            cookie_id: row.get::<String, _>("cookie_id"),
            item_id: row.get::<String, _>("item_id"),
            buyer_id: row.get::<String, _>("buyer_id"),
            amount: row.get::<String, _>("amount"),
            status: row.get::<String, _>("status"),
            last_shipped_at: row.get::<i64, _>("last_shipped_at"),
            last_msg_at: row.get::<i64, _>("last_msg_at"),
            created_at: row.get::<i64, _>("created_at"),
            updated_at: row.get::<i64, _>("updated_at"),
        }))
    }

    pub async fn upsert_business_order(
        &self,
        order_id: &str,
        cookie_id: &str,
        item_id: &str,
        buyer_id: &str,
        amount: &str,
        status: &str,
        last_shipped_at: i64,
        last_msg_at: i64,
    ) -> Result<BusinessOrder, StorageError> {
        let existing = self.get_business_order(cookie_id, order_id).await?;
        let now = unix_millis_now();
        let created_at = existing.as_ref().map(|row| row.created_at).unwrap_or(now);

        sqlx::query(
            r#"
            INSERT INTO biz_orders (
                order_id,
                cookie_id,
                item_id,
                buyer_id,
                amount,
                status,
                last_shipped_at,
                last_msg_at,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(cookie_id, order_id) DO UPDATE SET
                item_id = excluded.item_id,
                buyer_id = excluded.buyer_id,
                amount = excluded.amount,
                status = excluded.status,
                last_shipped_at = excluded.last_shipped_at,
                last_msg_at = excluded.last_msg_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(order_id)
        .bind(cookie_id)
        .bind(item_id)
        .bind(buyer_id)
        .bind(amount)
        .bind(status)
        .bind(last_shipped_at)
        .bind(last_msg_at)
        .bind(created_at)
        .bind(now)
        .execute(&self.accounts)
        .await?;

        self.get_business_order(cookie_id, order_id)
            .await?
            .ok_or_else(|| {
                StorageError::Io(std::io::Error::other("upserted business order missing"))
            })
    }

    pub async fn append_ai_conversation(
        &self,
        cookie_id: &str,
        chat_id: &str,
        item_id: &str,
        peer_uid: &str,
        role: &str,
        intent: &str,
        content: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO ai_conversations (
                cookie_id, chat_id, item_id, user_id, role, intent, content, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(cookie_id)
        .bind(chat_id)
        .bind(item_id)
        .bind(peer_uid)
        .bind(role)
        .bind(intent)
        .bind(content)
        .bind(unix_millis_now())
        .execute(&self.accounts)
        .await?;
        Ok(())
    }

    pub async fn list_recent_ai_conversations(
        &self,
        cookie_id: &str,
        chat_id: &str,
        limit: i64,
    ) -> Result<Vec<AiConversation>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                cookie_id,
                chat_id,
                item_id,
                user_id AS peer_uid,
                role,
                intent,
                content,
                created_at
            FROM ai_conversations
            WHERE cookie_id = ? AND chat_id = ?
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(cookie_id)
        .bind(chat_id)
        .bind(limit.max(1))
        .fetch_all(&self.accounts)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| AiConversation {
                id: row.get::<i64, _>("id"),
                cookie_id: row.get::<String, _>("cookie_id"),
                chat_id: row.get::<String, _>("chat_id"),
                item_id: row.get::<String, _>("item_id"),
                peer_uid: row.get::<String, _>("peer_uid"),
                role: row.get::<String, _>("role"),
                intent: row.get::<String, _>("intent"),
                content: row.get::<String, _>("content"),
                created_at: row.get::<f64, _>("created_at") as i64,
            })
            .collect())
    }

    pub async fn count_ai_messages_by_intent(
        &self,
        cookie_id: &str,
        chat_id: &str,
        role: &str,
        intent: &str,
    ) -> Result<i64, StorageError> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM ai_conversations
            WHERE cookie_id = ? AND chat_id = ? AND role = ? AND intent = ?
            "#,
        )
        .bind(cookie_id)
        .bind(chat_id)
        .bind(role)
        .bind(intent)
        .fetch_one(&self.accounts)
        .await?;
        Ok(count.max(0))
    }

    pub async fn record_default_reply_once(
        &self,
        cookie_id: &str,
        chat_id: &str,
        item_id: &str,
        reply_type: &str,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO default_reply_records (
                cookie_id, chat_id, item_id, reply_type, created_at
            )
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(cookie_id)
        .bind(chat_id)
        .bind(item_id)
        .bind(reply_type)
        .bind(unix_millis_now())
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn clear_default_reply_records(&self, cookie_id: &str) -> Result<u64, StorageError> {
        let result = sqlx::query(
            r#"
            DELETE FROM default_reply_records
            WHERE cookie_id = ?
              AND item_id = ''
              AND reply_type = 'default'
            "#,
        )
        .bind(cookie_id)
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn try_consume_item_reply_task_run(
        &self,
        cookie_id: &str,
        chat_id: &str,
        task_key: &str,
        limit: i64,
    ) -> Result<bool, StorageError> {
        let normalized_limit = normalize_item_reply_count(limit);
        let now = unix_millis_now();
        let result = sqlx::query(
            r#"
            INSERT INTO item_reply_task_runs (
                cookie_id, chat_id, task_key, run_count, created_at, updated_at
            )
            VALUES (?, ?, ?, 1, ?, ?)
            ON CONFLICT(cookie_id, chat_id, task_key) DO UPDATE SET
                run_count = item_reply_task_runs.run_count + 1,
                updated_at = excluded.updated_at
            WHERE item_reply_task_runs.run_count < ?
            "#,
        )
        .bind(cookie_id)
        .bind(chat_id)
        .bind(task_key)
        .bind(now)
        .bind(now)
        .bind(normalized_limit)
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn clear_item_reply_task_runs(
        &self,
        cookie_id: &str,
        task_key: &str,
    ) -> Result<u64, StorageError> {
        let result = sqlx::query(
            r#"
            DELETE FROM item_reply_task_runs
            WHERE cookie_id = ? AND task_key = ?
            "#,
        )
        .bind(cookie_id)
        .bind(task_key)
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn record_automation_message(
        &self,
        message_key: &str,
        cookie_id: &str,
        session_id: &str,
        peer_uid: &str,
        item_id: &str,
        kind: &str,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO automation_message_dedup (
                message_key, cookie_id, session_id, peer_uid, item_id, kind, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(message_key)
        .bind(cookie_id)
        .bind(session_id)
        .bind(peer_uid)
        .bind(item_id)
        .bind(kind)
        .bind(unix_millis_now())
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_old_automation_messages(
        &self,
        before_ms: i64,
    ) -> Result<u64, StorageError> {
        let result = sqlx::query("DELETE FROM automation_message_dedup WHERE created_at < ?")
            .bind(before_ms)
            .execute(&self.accounts)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn claim_next_card_by_pool(
        &self,
        cookie_id: &str,
        card_pool_tag: &str,
        order_id: &str,
    ) -> Result<Option<Card>, StorageError> {
        self.claim_next_card_by_pool_scoped(cookie_id, "account", card_pool_tag, order_id)
            .await
    }

    pub async fn claim_next_card_by_pool_scoped(
        &self,
        cookie_id: &str,
        pool_scope: &str,
        card_pool_tag: &str,
        order_id: &str,
    ) -> Result<Option<Card>, StorageError> {
        let resolved_cookie_id = self
            .resolve_card_pool_cookie_id(cookie_id, pool_scope)
            .await?;
        let mut tx = self.accounts.begin().await?;
        let row = sqlx::query(
            r#"
            SELECT id, cookie_id, card_key, card_content, card_pool_tag, reusable, used, used_at, used_order_id, created_at
                 , one_time_identity_active
            FROM cards
            WHERE cookie_id = ?
              AND card_pool_tag = ?
              AND TRIM(card_content) != ''
              AND (reusable = 1 OR one_time_identity_active = 1)
              AND (reusable = 1 OR used = 0)
              AND (reusable = 1 OR reserved_run_id IS NULL)
            ORDER BY reusable DESC, created_at ASC, id ASC
            LIMIT 1
            "#,
        )
        .bind(resolved_cookie_id)
        .bind(card_pool_tag)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };

        let card = card_from_row(row);
        if card.reusable {
            tx.commit().await?;
            return Ok(Some(card));
        }

        let now = unix_millis_now();
        let updated = sqlx::query(
            r#"
            UPDATE cards
            SET used = 1, used_at = ?, used_order_id = ?
            WHERE id = ?
              AND used = 0
              AND reserved_run_id IS NULL
              AND one_time_identity_active = 1
            "#,
        )
        .bind(now)
        .bind(order_id)
        .bind(card.id)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            tx.commit().await?;
            return Ok(None);
        }
        tx.commit().await?;

        Ok(Some(Card {
            used: true,
            used_at: now,
            used_order_id: order_id.to_string(),
            ..card
        }))
    }

    pub(crate) async fn resolve_card_pool_cookie_id(
        &self,
        cookie_id: &str,
        pool_scope: &str,
    ) -> Result<String, StorageError> {
        if normalize_card_pool_scope(pool_scope) == "global" {
            self.global_scope_key_for_account(cookie_id).await
        } else {
            Ok(cookie_id.to_string())
        }
    }

    pub async fn upsert_cache_sync_runtime_state(
        &self,
        status: &str,
        cached_uids: &[String],
        total_entries: u64,
        last_sync_at: f64,
    ) -> Result<CacheSyncRuntimeStatus, StorageError> {
        let cached_uids_json = serde_json::to_string(cached_uids)
            .map_err(|error| StorageError::Io(std::io::Error::other(error)))?;

        sqlx::query(
            r#"
            INSERT INTO cache_sync_runtime_state (
                id,
                status,
                cached_uids_json,
                total_entries,
                last_sync_at,
                updated_at
            )
            VALUES (1, ?, ?, ?, ?, unixepoch('subsec'))
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                cached_uids_json = excluded.cached_uids_json,
                total_entries = excluded.total_entries,
                last_sync_at = excluded.last_sync_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(status)
        .bind(cached_uids_json)
        .bind(total_entries as i64)
        .bind(last_sync_at)
        .execute(&self.xianyu)
        .await?;

        self.get_cache_sync_runtime_state().await
    }

    pub async fn get_cache_sync_runtime_state(
        &self,
    ) -> Result<CacheSyncRuntimeStatus, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT status, cached_uids_json, total_entries, last_sync_at
            FROM cache_sync_runtime_state
            WHERE id = 1
            "#,
        )
        .fetch_optional(&self.xianyu)
        .await?;

        Ok(match row {
            Some(row) => CacheSyncRuntimeStatus {
                status: row.get::<String, _>("status"),
                cached_uids: parse_json_vec_strings(
                    row.get::<String, _>("cached_uids_json").as_str(),
                ),
                total_entries: row.get::<i64, _>("total_entries").max(0) as u64,
                last_sync_at: row.get::<f64, _>("last_sync_at"),
            },
            None => CacheSyncRuntimeStatus::default(),
        })
    }

    pub async fn upsert_delete_runtime_state(
        &self,
        status: &DeleteRuntimeStatus,
    ) -> Result<DeleteRuntimeStatus, StorageError> {
        let running_job_ids_json = serde_json::to_string(&status.running_job_ids)
            .map_err(|error| StorageError::Io(std::io::Error::other(error)))?;

        sqlx::query(
            r#"
            INSERT INTO comment_delete_runtime_state (
                id,
                running_job_ids_json,
                running_count,
                pending_count,
                retryable_count,
                persisted_running_count,
                runtime_ready,
                last_poll_at,
                last_runtime_error,
                last_recovered_count,
                claim_batch_size,
                action_capacity,
                updated_at
            )
            VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'))
            ON CONFLICT(id) DO UPDATE SET
                running_job_ids_json = excluded.running_job_ids_json,
                running_count = excluded.running_count,
                pending_count = excluded.pending_count,
                retryable_count = excluded.retryable_count,
                persisted_running_count = excluded.persisted_running_count,
                runtime_ready = excluded.runtime_ready,
                last_poll_at = excluded.last_poll_at,
                last_runtime_error = excluded.last_runtime_error,
                last_recovered_count = excluded.last_recovered_count,
                claim_batch_size = excluded.claim_batch_size,
                action_capacity = excluded.action_capacity,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(running_job_ids_json)
        .bind(status.running_count as i64)
        .bind(status.pending_count as i64)
        .bind(status.retryable_count as i64)
        .bind(status.persisted_running_count as i64)
        .bind(if status.runtime_ready { 1_i64 } else { 0_i64 })
        .bind(status.last_poll_at)
        .bind(&status.last_runtime_error)
        .bind(status.last_recovered_count as i64)
        .bind(status.claim_batch_size as i64)
        .bind(status.action_capacity as i64)
        .execute(&self.xianyu)
        .await?;

        self.get_delete_runtime_state().await
    }

    pub async fn get_delete_runtime_state(&self) -> Result<DeleteRuntimeStatus, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT
                running_job_ids_json,
                running_count,
                pending_count,
                retryable_count,
                persisted_running_count,
                runtime_ready,
                last_poll_at,
                last_runtime_error,
                last_recovered_count,
                claim_batch_size,
                action_capacity
            FROM comment_delete_runtime_state
            WHERE id = 1
            "#,
        )
        .fetch_optional(&self.xianyu)
        .await?;

        Ok(match row {
            Some(row) => DeleteRuntimeStatus {
                running_job_ids: parse_json_vec_i64(
                    row.get::<String, _>("running_job_ids_json").as_str(),
                ),
                running_count: row.get::<i64, _>("running_count").max(0) as u64,
                pending_count: row.get::<i64, _>("pending_count").max(0) as u64,
                retryable_count: row.get::<i64, _>("retryable_count").max(0) as u64,
                persisted_running_count: row.get::<i64, _>("persisted_running_count").max(0) as u64,
                runtime_ready: row.get::<i64, _>("runtime_ready") != 0,
                last_poll_at: row.get::<f64, _>("last_poll_at"),
                last_runtime_error: row.get::<String, _>("last_runtime_error"),
                last_recovered_count: row.get::<i64, _>("last_recovered_count").max(0) as u64,
                claim_batch_size: row.get::<i64, _>("claim_batch_size").max(0) as u64,
                action_capacity: row.get::<i64, _>("action_capacity").max(0) as u64,
            },
            None => DeleteRuntimeStatus::default(),
        })
    }
}

fn default_kind(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "text".to_string()
    } else {
        trimmed.to_string()
    }
}

fn keyword_from_row(row: SqliteRow) -> Keyword {
    Keyword {
        id: row.get::<i64, _>("id"),
        cookie_id: row.get::<String, _>("cookie_id"),
        keyword: row.get::<String, _>("keyword"),
        reply: row.get::<String, _>("reply"),
        kind: row.get::<String, _>("type"),
        item_id: row.get::<String, _>("item_id"),
        enabled: row.get::<i64, _>("enabled") != 0,
        task_group_id: row.get::<String, _>("task_group_id"),
        created_at: row.get::<f64, _>("created_at") as i64,
    }
}

fn public_keyword(mut keyword: Keyword) -> Keyword {
    keyword.cookie_id = public_cookie_id(&keyword.cookie_id);
    keyword
}

fn item_reply_from_row(row: SqliteRow) -> ItemReply {
    ItemReply {
        id: row.get::<i64, _>("id"),
        cookie_id: row.get::<String, _>("cookie_id"),
        item_id: row.get::<String, _>("item_id"),
        reply: row.get::<String, _>("reply"),
        kind: row.get::<String, _>("type"),
        enabled: row.get::<i64, _>("enabled") != 0,
        auto_recall: row.get::<i64, _>("auto_recall") != 0,
        task_group_id: row.get::<String, _>("task_group_id"),
        reply_count: normalize_item_reply_count(row.get::<i64, _>("reply_count")),
        created_at: row.get::<f64, _>("created_at") as i64,
    }
}

fn public_item_reply(mut item_reply: ItemReply) -> ItemReply {
    item_reply.cookie_id = public_cookie_id(&item_reply.cookie_id);
    item_reply
}

fn item_reply_mode_from_row(row: SqliteRow) -> ItemReplyMode {
    ItemReplyMode {
        cookie_id: row.get::<String, _>("cookie_id"),
        item_id: row.get::<String, _>("item_id"),
        reply_mode: ReplyMode::from_item_raw(row.get::<String, _>("reply_mode").as_str()),
        updated_at: row_i64_compat(&row, "updated_at"),
    }
}

fn public_item_reply_mode(mut item_reply_mode: ItemReplyMode) -> ItemReplyMode {
    item_reply_mode.cookie_id = public_cookie_id(&item_reply_mode.cookie_id);
    item_reply_mode
}

fn card_from_row(row: SqliteRow) -> Card {
    let reusable = row.get::<i64, _>("reusable") != 0;
    let one_time_identity_active = row.get::<i64, _>("one_time_identity_active") != 0;
    Card {
        id: row.get::<i64, _>("id"),
        cookie_id: row.get::<String, _>("cookie_id"),
        card_key: row.get::<String, _>("card_key"),
        card_content: row.get::<String, _>("card_content"),
        card_pool_tag: row.get::<String, _>("card_pool_tag"),
        reusable,
        used: row.get::<i64, _>("used") != 0,
        quarantined: !reusable && !one_time_identity_active,
        used_at: row.get::<f64, _>("used_at") as i64,
        used_order_id: row.get::<String, _>("used_order_id"),
        created_at: row.get::<f64, _>("created_at") as i64,
    }
}

fn public_card(mut card: Card) -> Card {
    card.cookie_id = public_cookie_id(&card.cookie_id);
    card
}

fn delivery_rule_from_row(row: SqliteRow) -> DeliveryRule {
    DeliveryRule {
        id: row.get::<i64, _>("id"),
        cookie_id: row.get::<String, _>("cookie_id"),
        keyword: row.get::<String, _>("keyword"),
        item_id: row.get::<String, _>("item_id"),
        card_pool_tag: row.get::<String, _>("card_pool_tag"),
        card_pool_scope: normalize_card_pool_scope(
            row.get::<String, _>("card_pool_scope").as_str(),
        )
        .to_string(),
        enabled: row.get::<i64, _>("enabled") != 0,
        created_at: row.get::<f64, _>("created_at") as i64,
    }
}

fn public_delivery_rule(mut delivery_rule: DeliveryRule) -> DeliveryRule {
    delivery_rule.cookie_id = public_cookie_id(&delivery_rule.cookie_id);
    delivery_rule
}

fn ai_settings_from_row(row: &SqliteRow, cookie_id: &str) -> AiSettings {
    let reply_mode = ai_row_reply_mode(row);
    AiSettings {
        cookie_id: cookie_id.to_string(),
        reply_mode,
        ai_enabled: reply_mode.ai_enabled(),
        auto_reply_enabled: reply_mode.auto_reply_enabled(),
        auto_ship_enabled: ai_row_bool(row, "auto_ship_enabled"),
        api_type: row.get::<String, _>("api_type"),
        api_key: row.get::<String, _>("api_key"),
        base_url: row.get::<String, _>("base_url"),
        model_name: row.get::<String, _>("model_name"),
        max_bargain_rounds: row.get::<i64, _>("max_bargain_rounds"),
        max_discount_percent: row.get::<f64, _>("max_discount_percent"),
        max_discount_amount: row.get::<f64, _>("max_discount_amount"),
        custom_prompts: parse_json_map(row.get::<String, _>("custom_prompts").as_str()),
        updated_at: ai_row_updated_at(row),
    }
}

fn ai_row_bool(row: &SqliteRow, column: &str) -> bool {
    row.get::<i64, _>(column) != 0
}

fn ai_row_reply_mode(row: &SqliteRow) -> ReplyMode {
    let reply_mode = ReplyMode::from_account_raw(row.get::<String, _>("reply_mode").as_str());
    if reply_mode != ReplyMode::Off {
        return reply_mode;
    }
    if ai_row_bool(row, "ai_enabled") {
        return ReplyMode::Ai;
    }
    if ai_row_bool(row, "auto_reply_enabled") {
        return ReplyMode::Default;
    }
    ReplyMode::Off
}

fn ai_row_updated_at(row: &SqliteRow) -> i64 {
    row_i64_compat(row, "updated_at")
}

fn row_i64_compat(row: &SqliteRow, column: &str) -> i64 {
    row.try_get::<i64, _>(column)
        .or_else(|_| row.try_get::<f64, _>(column).map(|value| value as i64))
        .unwrap_or_default()
}

fn resolve_next_reply_mode(
    _cookie_id: &str,
    preserved: ReplyMode,
    explicit: Option<ReplyMode>,
    ai_enabled: Option<bool>,
    auto_reply_enabled: Option<bool>,
) -> ReplyMode {
    if let Some(mode) = explicit {
        return mode.normalize_account();
    }

    if matches!(ai_enabled, Some(true)) {
        return ReplyMode::Ai;
    }

    if matches!(auto_reply_enabled, Some(false)) {
        return ReplyMode::Off;
    }

    if matches!(ai_enabled, Some(false)) && preserved.normalize_account() == ReplyMode::Ai {
        return if matches!(auto_reply_enabled, Some(true)) {
            ReplyMode::Default
        } else {
            ReplyMode::Off
        };
    }

    if matches!(auto_reply_enabled, Some(true)) {
        let normalized = preserved.normalize_account();
        return match normalized {
            ReplyMode::Ai | ReplyMode::Keyword | ReplyMode::Default => normalized,
            _ => ReplyMode::Default,
        };
    }

    preserved.normalize_account()
}

fn ai_payload_has_config(payload: &AiSettingsUpdatePayload) -> bool {
    payload.api_type.is_some()
        || payload.api_key.is_some()
        || payload.base_url.is_some()
        || payload.model_name.is_some()
        || payload.max_bargain_rounds.is_some()
        || payload.max_discount_percent.is_some()
        || payload.max_discount_amount.is_some()
        || payload.custom_prompts.is_some()
}

fn normalize_card_pool_scope(scope: &str) -> &'static str {
    if scope.trim().eq_ignore_ascii_case("global") {
        "global"
    } else {
        "account"
    }
}

fn resolve_card_pool_cookie_id<'a>(cookie_id: &'a str, pool_scope: &str) -> &'a str {
    if normalize_card_pool_scope(pool_scope) == "global" {
        GLOBAL_AI_SETTINGS_KEY
    } else {
        cookie_id
    }
}

struct NormalizedCardWrite {
    card_key: String,
    card_content: String,
    card_pool_tag: String,
    reusable: bool,
}

fn map_card_write_error(error: sqlx::Error, owner_scope_key: &str, pool_tag: &str) -> StorageError {
    if let sqlx::Error::Database(database) = &error {
        let message = database.message();
        if message.contains("ux_cards_one_time_owner_pool_content")
            || message.contains("cards.cookie_id, cards.card_pool_tag")
        {
            return StorageError::DuplicateOneTimeCard {
                owner_scope_key: owner_scope_key.to_string(),
                pool_tag: pool_tag.to_string(),
            };
        }
    }
    StorageError::Sqlx(error)
}
