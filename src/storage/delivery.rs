use std::{collections::HashMap, io, str::FromStr};

use sqlx::{sqlite::SqliteRow, QueryBuilder, Row, Sqlite};

use crate::models::delivery::{
    CancelDeliveryRunOutcome, CardReservation, DeliveryAlert, DeliveryCompletionOutcome,
    DeliveryConfigurationRecord, DeliveryDiscoveryAlert, DeliveryRun, DeliveryRunCard,
    DeliveryRunStatus, DeliveryRunSummary, NewDeliveryRun, PendingDeliveryOrderMismatch,
    SaveDeliveryConfigurationRequest, SideEffectClaim, UpdateDeliveryConfigurationRequest,
};

use super::{begin_immediate_with_retry, unix_millis_now, StorageError, StorageState};

impl StorageState {
    pub async fn create_delivery_configuration(
        &self,
        input: &SaveDeliveryConfigurationRequest,
        pool_tag: &str,
        cards: &[String],
    ) -> Result<DeliveryConfigurationRecord, StorageError> {
        let now = unix_millis_now();
        let mut pool_tags = normalize_pool_tags(&input.pool_tags, &input.pool_tag);
        let fixed_pool_tags = normalize_pool_tags(&input.fixed_pool_tags, "");
        if !pool_tags.is_empty() && !fixed_pool_tags.is_empty() {
            return Err(StorageError::Conflict {
                message: "固定卡密池和一次性卡密池不能同时配置".to_string(),
            });
        }
        if pool_tags.is_empty() && !pool_tag.trim().is_empty() {
            pool_tags.push(pool_tag.trim().to_string());
        }
        // Fixed pools and one-time pools both live under card pool scope (usually global).
        // Previously only one-time pools applied input.pool_scope, so fixed-only configs
        // were saved as account-scope and later failed to load global fixed cards.
        let references_card_pools = !pool_tags.is_empty() || !fixed_pool_tags.is_empty();
        let selected_pool_tag = pool_tags.first().map(String::as_str).unwrap_or("");
        let selected_pool_scope = if references_card_pools {
            normalize_pool_scope(&input.pool_scope)?
        } else {
            "account"
        };
        let mut tx =
            begin_immediate_with_retry(&self.accounts, "create_delivery_configuration").await?;
        let cursor = sqlx::query(
            r#"
            INSERT INTO delivery_rules (
                cookie_id, keyword, item_id, card_pool_tag, card_pool_scope,
                fixed_message, one_time_enabled, enabled, needs_repair, created_at
            ) VALUES (?, '', ?, ?, ?, ?, ?, ?, 0, ?)
            "#,
        )
        .bind(input.uid.trim())
        .bind(input.item_id.trim())
        .bind(selected_pool_tag)
        .bind(selected_pool_scope)
        .bind(input.fixed_message.trim())
        .bind(if input.one_time_enabled { 1_i64 } else { 0_i64 })
        .bind(if input.enabled { 1_i64 } else { 0_i64 })
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let id = cursor.last_insert_rowid();
        replace_delivery_rule_pools(&mut tx, id, &pool_tags, now).await?;
        replace_delivery_rule_fixed_pools(&mut tx, id, &fixed_pool_tags, now).await?;

        if !cards.is_empty() && input.pool_tags.is_empty() && input.pool_tag.trim().is_empty() {
            insert_delivery_cards(&mut tx, input.uid.trim(), selected_pool_tag, cards, now).await?;
        }
        enable_auto_ship(&mut tx, input.uid.trim(), now).await?;
        tx.commit().await?;
        self.delivery_configuration_record(id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: "自动发货配置创建后未找到".to_string(),
            })
    }

    pub async fn list_delivery_configuration_records(
        &self,
        uid: &str,
    ) -> Result<Vec<DeliveryConfigurationRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, cookie_id, item_id, fixed_message, one_time_enabled,
                   enabled, needs_repair, card_pool_scope, card_pool_tag, created_at,
                   COALESCE((SELECT GROUP_CONCAT(pool_tag, char(31)) FROM delivery_rule_card_pools p WHERE p.rule_id = delivery_rules.id ORDER BY position), '') AS pool_tags,
                   COALESCE((SELECT GROUP_CONCAT(pool_tag, char(31)) FROM delivery_rule_fixed_card_pools p WHERE p.rule_id = delivery_rules.id ORDER BY position), '') AS fixed_pool_tags
            FROM delivery_rules
            WHERE cookie_id = ?
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .bind(uid.trim())
        .fetch_all(&self.accounts)
        .await?;
        Ok(rows
            .into_iter()
            .map(delivery_configuration_from_row)
            .collect())
    }

    pub async fn delivery_configuration_record(
        &self,
        id: i64,
    ) -> Result<Option<DeliveryConfigurationRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, cookie_id, item_id, fixed_message, one_time_enabled,
                   enabled, needs_repair, card_pool_scope, card_pool_tag, created_at,
                   COALESCE((SELECT GROUP_CONCAT(pool_tag, char(31)) FROM delivery_rule_card_pools p WHERE p.rule_id = delivery_rules.id ORDER BY position), '') AS pool_tags,
                   COALESCE((SELECT GROUP_CONCAT(pool_tag, char(31)) FROM delivery_rule_fixed_card_pools p WHERE p.rule_id = delivery_rules.id ORDER BY position), '') AS fixed_pool_tags
            FROM delivery_rules
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.accounts)
        .await?;
        Ok(row.map(delivery_configuration_from_row))
    }

    pub async fn available_delivery_card_count(
        &self,
        configuration: &DeliveryConfigurationRecord,
    ) -> Result<i64, StorageError> {
        if configuration.pool_tags.is_empty() {
            return Ok(0);
        }
        let pool_cookie_id = self
            .resolve_card_pool_cookie_id(&configuration.uid, &configuration.pool_scope)
            .await?;
        let mut builder = QueryBuilder::<Sqlite>::new(if configuration.one_time_enabled {
            r#"
            SELECT COUNT(*) FROM cards
            WHERE cookie_id =
            "#
        } else {
            r#"SELECT COUNT(*) FROM cards WHERE cookie_id = "#
        });
        builder
            .push_bind(pool_cookie_id)
            .push(" AND card_pool_tag IN (");
        let mut separated = builder.separated(", ");
        for tag in &configuration.pool_tags {
            separated.push_bind(tag);
        }
        separated.push_unseparated(")");
        builder.push(if configuration.one_time_enabled {
            r#" AND reusable = 0
              AND one_time_identity_active = 1
              AND used = 0 AND reserved_run_id IS NULL AND TRIM(card_content) <> ''
            "#
        } else {
            " AND reusable = 1 AND TRIM(card_content) <> ''"
        });
        let count = builder
            .build_query_scalar::<i64>()
            .fetch_one(&self.accounts)
            .await?;
        Ok(count)
    }

    pub async fn delivery_card_pool_count(
        &self,
        uid: &str,
        pool_scope: &str,
        pool_tag: &str,
        one_time_enabled: bool,
    ) -> Result<i64, StorageError> {
        let pool_cookie_id = self
            .resolve_card_pool_cookie_id(uid, normalize_pool_scope(pool_scope)?)
            .await?;
        let count = sqlx::query_scalar::<_, i64>(if one_time_enabled {
            r#"
            SELECT COUNT(*) FROM cards
            WHERE cookie_id = ? AND card_pool_tag = ? AND reusable = 0
              AND one_time_identity_active = 1
              AND used = 0 AND reserved_run_id IS NULL AND TRIM(card_content) <> ''
            "#
        } else {
            r#"
            SELECT COUNT(*) FROM cards
            WHERE cookie_id = ? AND card_pool_tag = ? AND reusable = 1
              AND TRIM(card_content) <> ''
            "#
        })
        .bind(pool_cookie_id)
        .bind(pool_tag.trim())
        .fetch_one(&self.accounts)
        .await?;
        Ok(count)
    }

    pub async fn fixed_delivery_card_contents(
        &self,
        configuration: &DeliveryConfigurationRecord,
    ) -> Result<Vec<String>, StorageError> {
        let pool_cookie_id = self
            .resolve_card_pool_cookie_id(&configuration.uid, &configuration.pool_scope)
            .await?;
        let fallback_cookie = if configuration
            .pool_scope
            .trim()
            .eq_ignore_ascii_case("global")
        {
            None
        } else {
            self.resolve_card_pool_cookie_id(&configuration.uid, "global")
                .await
                .ok()
                .filter(|value| value != &pool_cookie_id)
        };
        let mut contents = Vec::new();
        for pool_tag in &configuration.fixed_pool_tags {
            let mut content = sqlx::query_scalar::<_, String>(
                r#"
            SELECT card_content FROM cards
            WHERE cookie_id = ? AND card_pool_tag = ? AND reusable = 1
              AND TRIM(card_content) <> ''
            ORDER BY created_at ASC, id ASC
            LIMIT 1
            "#,
            )
            .bind(&pool_cookie_id)
            .bind(pool_tag)
            .fetch_optional(&self.accounts)
            .await?;
            if content.is_none() {
                if let Some(global_cookie) = fallback_cookie.as_ref() {
                    content = sqlx::query_scalar::<_, String>(
                        r#"
                    SELECT card_content FROM cards
                    WHERE cookie_id = ? AND card_pool_tag = ? AND reusable = 1
                      AND TRIM(card_content) <> ''
                    ORDER BY created_at ASC, id ASC
                    LIMIT 1
                    "#,
                    )
                    .bind(global_cookie)
                    .bind(pool_tag)
                    .fetch_optional(&self.accounts)
                    .await?;
                }
            }
            if let Some(content) =
                content.filter(|value| !contents.iter().any(|seen| seen == value))
            {
                contents.push(content);
            }
        }
        Ok(contents)
    }

    pub async fn fixed_delivery_card_contents_for_run(
        &self,
        run: &DeliveryRun,
    ) -> Result<Vec<String>, StorageError> {
        let primary_cookie = if run.pool_owner_key.trim().is_empty() {
            self.resolve_card_pool_cookie_id(&run.uid, &run.pool_scope_snapshot)
                .await?
        } else {
            run.pool_owner_key.clone()
        };
        // Fallback for historical mis-saved account-scope fixed configs whose cards
        // actually live in the owner global pool.
        let fallback_cookie = if run
            .pool_scope_snapshot
            .trim()
            .eq_ignore_ascii_case("global")
        {
            None
        } else {
            self.resolve_card_pool_cookie_id(&run.uid, "global")
                .await
                .ok()
                .filter(|value| value != &primary_cookie)
        };
        let mut contents = Vec::new();
        for pool_tag in &run.fixed_pool_tags {
            let mut content = sqlx::query_scalar::<_, String>(
                r#"
                SELECT card_content FROM cards
                WHERE cookie_id = ? AND card_pool_tag = ? AND reusable = 1
                  AND TRIM(card_content) <> ''
                ORDER BY created_at ASC, id ASC
                LIMIT 1
                "#,
            )
            .bind(&primary_cookie)
            .bind(pool_tag)
            .fetch_optional(&self.accounts)
            .await?;
            if content.is_none() {
                if let Some(global_cookie) = fallback_cookie.as_ref() {
                    content = sqlx::query_scalar::<_, String>(
                        r#"
                        SELECT card_content FROM cards
                        WHERE cookie_id = ? AND card_pool_tag = ? AND reusable = 1
                          AND TRIM(card_content) <> ''
                        ORDER BY created_at ASC, id ASC
                        LIMIT 1
                        "#,
                    )
                    .bind(global_cookie)
                    .bind(pool_tag)
                    .fetch_optional(&self.accounts)
                    .await?;
                }
            }
            if let Some(content) =
                content.filter(|value| !contents.iter().any(|seen| seen == value))
            {
                contents.push(content);
            }
        }
        Ok(contents)
    }

    pub async fn available_delivery_card_counts(
        &self,
        uid: &str,
        pool_tags: &[String],
    ) -> Result<HashMap<String, i64>, StorageError> {
        if pool_tags.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT card_pool_tag, COUNT(*) AS available_count
            FROM cards
            WHERE cookie_id =
            "#,
        );
        builder
            .push_bind(uid.trim())
            .push(" AND card_pool_tag IN (");
        let mut separated = builder.separated(", ");
        for pool_tag in pool_tags {
            separated.push_bind(pool_tag);
        }
        separated.push_unseparated(")");
        builder.push(
            r#"
            AND reusable = 0 AND one_time_identity_active = 1
            AND used = 0 AND reserved_run_id IS NULL
            AND TRIM(card_content) <> ''
            GROUP BY card_pool_tag
            "#,
        );
        let rows = builder.build().fetch_all(&self.accounts).await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("card_pool_tag"),
                    row.get::<i64, _>("available_count"),
                )
            })
            .collect())
    }

    pub async fn update_delivery_configuration(
        &self,
        id: i64,
        input: &UpdateDeliveryConfigurationRequest,
        cards: &[String],
    ) -> Result<Option<DeliveryConfigurationRecord>, StorageError> {
        let Some(current) = self.delivery_configuration_record(id).await? else {
            return Ok(None);
        };
        if input
            .uid
            .as_deref()
            .is_some_and(|value| value.trim() != current.uid)
            || input
                .item_id
                .as_deref()
                .is_some_and(|value| value.trim() != current.item_id)
        {
            return Err(StorageError::Conflict {
                message: "自动发货配置不能更换账号或商品".to_string(),
            });
        }
        let next_pool_scope = input
            .pool_scope
            .as_deref()
            .map(normalize_pool_scope)
            .transpose()?
            .unwrap_or(current.pool_scope.as_str());
        let next_pool_tags = input
            .pool_tags
            .as_ref()
            .map(|tags| normalize_pool_tags(tags, input.pool_tag.as_deref().unwrap_or("")))
            .unwrap_or_else(|| {
                input
                    .pool_tag
                    .as_deref()
                    .map(|tag| normalize_pool_tags(&[], tag))
                    .unwrap_or_else(|| current.pool_tags.clone())
            });
        let next_fixed_pool_tags = input
            .fixed_pool_tags
            .clone()
            .unwrap_or_else(|| current.fixed_pool_tags.clone());
        if !next_pool_tags.is_empty() && !next_fixed_pool_tags.is_empty() {
            return Err(StorageError::Conflict {
                message: "固定卡密池和一次性卡密池不能同时配置".to_string(),
            });
        }
        let next_pool_tag = next_pool_tags.first().map(String::as_str).unwrap_or("");
        let next_one_time_enabled = !next_pool_tags.is_empty();
        let next_fixed_message = input
            .fixed_message
            .as_deref()
            .unwrap_or(&current.fixed_message)
            .trim();
        if next_pool_tags.is_empty()
            && next_fixed_pool_tags.is_empty()
            && next_fixed_message.is_empty()
        {
            return Err(StorageError::Conflict {
                message: "请至少配置一次性卡密池、固定卡密池或固定说明".to_string(),
            });
        }
        let next_pool_cookie_id = self
            .resolve_card_pool_cookie_id(&current.uid, next_pool_scope)
            .await?;
        let mut tx =
            begin_immediate_with_retry(&self.accounts, "update_delivery_configuration").await?;
        let row = sqlx::query(
            r#"
            SELECT id
            FROM delivery_rules WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(_) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let now = unix_millis_now();
        if !cards.is_empty() && next_pool_scope == "account" && next_pool_tag == current.pool_tag {
            insert_delivery_cards(&mut tx, &next_pool_cookie_id, next_pool_tag, cards, now).await?;
        }
        if !next_pool_tags.is_empty() {
            let mut count_query =
                QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM cards WHERE cookie_id = ");
            count_query
                .push_bind(&next_pool_cookie_id)
                .push(" AND card_pool_tag IN (");
            let mut separated = count_query.separated(", ");
            for pool_tag in &next_pool_tags {
                separated.push_bind(pool_tag);
            }
            separated.push_unseparated(")");
            count_query.push(
                " AND reusable = 0 AND one_time_identity_active = 1 AND used = 0 AND reserved_run_id IS NULL AND TRIM(card_content) <> ''",
            );
            let available = count_query
                .build_query_scalar::<i64>()
                .fetch_one(&mut *tx)
                .await?;
            if available == 0 {
                return Err(StorageError::Conflict {
                    message: "所选卡池没有可用的一次性卡密".to_string(),
                });
            }
        }
        let mut builder = QueryBuilder::<Sqlite>::new("UPDATE delivery_rules SET ");
        let mut changed = false;
        let mut first = true;
        let mut assignment = |builder: &mut QueryBuilder<Sqlite>, column: &str| {
            if !first {
                builder.push(", ");
            }
            first = false;
            builder.push(column).push(" = ");
        };
        if let Some(message) = input.fixed_message.as_deref() {
            assignment(&mut builder, "fixed_message");
            builder.push_bind(message.trim());
            changed = true;
        }
        if input.pool_scope.is_some() {
            assignment(&mut builder, "card_pool_scope");
            builder.push_bind(next_pool_scope);
            changed = true;
        }
        if input.pool_tag.is_some() {
            assignment(&mut builder, "card_pool_tag");
            builder.push_bind(next_pool_tag);
            changed = true;
        }
        if input.one_time_enabled.is_some() || input.pool_tags.is_some() || input.pool_tag.is_some()
        {
            assignment(&mut builder, "one_time_enabled");
            builder.push_bind(if next_one_time_enabled { 1_i64 } else { 0_i64 });
            changed = true;
        }
        if let Some(enabled) = input.enabled {
            assignment(&mut builder, "enabled");
            builder.push_bind(if enabled { 1_i64 } else { 0_i64 });
            changed = true;
        }
        if changed {
            builder.push(" WHERE id = ").push_bind(id);
            builder.build().execute(&mut *tx).await?;
        }
        if input.pool_tags.is_some() || input.pool_tag.is_some() {
            replace_delivery_rule_pools(&mut tx, id, &next_pool_tags, now).await?;
        }
        if input.fixed_pool_tags.is_some() {
            replace_delivery_rule_fixed_pools(&mut tx, id, &next_fixed_pool_tags, now).await?;
        }
        if input.enabled == Some(true) {
            enable_auto_ship(&mut tx, &current.uid, now).await?;
        }
        tx.commit().await?;
        self.delivery_configuration_record(id).await
    }

    pub async fn add_delivery_cards(
        &self,
        configuration: &DeliveryConfigurationRecord,
        cards: &[String],
    ) -> Result<i64, StorageError> {
        let pool_cookie_id = self
            .resolve_card_pool_cookie_id(&configuration.uid, &configuration.pool_scope)
            .await?;
        let mut tx = begin_immediate_with_retry(&self.accounts, "add_delivery_cards").await?;
        let inserted = insert_delivery_cards(
            &mut tx,
            &pool_cookie_id,
            &configuration.pool_tag,
            cards,
            unix_millis_now(),
        )
        .await?;
        tx.commit().await?;
        Ok(inserted)
    }

    pub async fn delete_delivery_configuration(
        &self,
        configuration: &DeliveryConfigurationRecord,
    ) -> Result<bool, StorageError> {
        let mut tx =
            begin_immediate_with_retry(&self.accounts, "delete_delivery_configuration").await?;
        let result = sqlx::query("DELETE FROM delivery_rules WHERE id = ?")
            .bind(configuration.id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 1 {
            let remaining = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM delivery_rules WHERE cookie_id = ?",
            )
            .bind(configuration.uid.trim())
            .fetch_one(&mut *tx)
            .await?;
            if remaining == 0 {
                set_auto_ship_enabled(&mut tx, configuration.uid.trim(), false, unix_millis_now())
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn list_delivery_runs_summary(
        &self,
        uids: &[String],
        configuration_id: Option<i64>,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<DeliveryRunSummary>, StorageError> {
        let page_size = page_size.clamp(1, 200);
        let offset = page
            .checked_sub(1)
            .filter(|value| *value >= 0)
            .and_then(|value| value.checked_mul(page_size))
            .ok_or_else(|| StorageError::Conflict {
                message: "执行记录分页参数超出范围".to_string(),
            })?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let status = status.filter(|value| !value.trim().is_empty());
        let query_prefix = if configuration_id.is_none() && status.is_none() {
            r#"
            SELECT id, uid, rule_id, order_id, buyer_id, item_id, purchase_quantity,
                   required_card_count, status, attempt_count, last_error_code,
                   last_error_message, created_at, updated_at
            FROM delivery_runs INDEXED BY idx_delivery_runs_updated_id_uid WHERE 1 = 1
            "#
        } else {
            r#"
            SELECT id, uid, rule_id, order_id, buyer_id, item_id, purchase_quantity,
                   required_card_count, status, attempt_count, last_error_code,
                   last_error_message, created_at, updated_at
            FROM delivery_runs WHERE 1 = 1
            "#
        };
        let mut builder = QueryBuilder::<Sqlite>::new(query_prefix);
        builder.push(" AND uid IN (");
        let mut separated = builder.separated(", ");
        for uid in uids {
            separated.push_bind(uid.trim());
        }
        separated.push_unseparated(")");
        if let Some(configuration_id) = configuration_id {
            builder.push(" AND rule_id = ").push_bind(configuration_id);
        }
        if let Some(status) = status {
            builder.push(" AND status = ").push_bind(status.trim());
        }
        builder
            .push(" ORDER BY updated_at DESC, id DESC LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(offset);
        let rows = builder.build().fetch_all(&self.accounts).await?;
        Ok(rows
            .into_iter()
            .map(delivery_run_summary_from_row)
            .collect())
    }

    pub async fn delivery_run_summary(
        &self,
        id: i64,
    ) -> Result<Option<DeliveryRunSummary>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, uid, rule_id, order_id, buyer_id, item_id, purchase_quantity,
                   required_card_count, status, attempt_count, last_error_code,
                   last_error_message, created_at, updated_at
            FROM delivery_runs WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.accounts)
        .await?;
        Ok(row.map(delivery_run_summary_from_row))
    }

    pub async fn retry_delivery_run(
        &self,
        id: i64,
        uid: &str,
    ) -> Result<Option<DeliveryRunSummary>, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "retry_delivery_run").await?;
        let row = sqlx::query(
            r#"
            SELECT status, last_error_code, consigned_at, delivery_send_claimed_at
            FROM delivery_runs
            WHERE id = ? AND uid = ?
            "#,
        )
        .bind(id)
        .bind(uid.trim())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let status = row.get::<String, _>("status");
        if !matches!(status.as_str(), "failed" | "retrying" | "manual_required") {
            tx.commit().await?;
            return Ok(None);
        }
        let error_code = row.get::<String, _>("last_error_code");
        let send_claimed_at = row.get::<i64, _>("delivery_send_claimed_at");
        if requires_manual_delivery_confirmation(&error_code) || send_claimed_at > 0 {
            return Err(StorageError::Conflict {
                message: "该任务发送结果不确定，必须先人工确认，不能直接重试".to_string(),
            });
        }
        let next_status = if row.get::<i64, _>("consigned_at") > 0 {
            "sending"
        } else {
            "pending"
        };
        let now = unix_millis_now();
        let result = sqlx::query(
            r#"
            UPDATE delivery_runs
            SET status = ?, next_retry_at = 0, delivery_send_claimed_at = 0,
                last_error_code = '', last_error_message = '', updated_at = ?
            WHERE id = ? AND uid = ? AND status = ? AND last_error_code = ?
            "#,
        )
        .bind(next_status)
        .bind(now)
        .bind(id)
        .bind(uid.trim())
        .bind(&status)
        .bind(&error_code)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        tx.commit().await?;
        self.delivery_run_summary(id).await
    }

    pub async fn ensure_delivery_run(
        &self,
        input: NewDeliveryRun,
    ) -> Result<DeliveryRun, StorageError> {
        let now = unix_millis_now();
        let mut tx = begin_immediate_with_retry(&self.accounts, "ensure_delivery_run").await?;
        let snapshot = delivery_run_snapshot_from_rule_tx(&mut tx, self, &input, true).await?;
        sqlx::query(
            r#"
            INSERT INTO delivery_runs (
                uid, rule_id, order_id, buyer_id, item_id, pool_owner_key,
                pool_scope_snapshot, one_time_pool_tags_json, fixed_pool_tags_json,
                fixed_message_snapshot, fixed_message_snapshot_captured,
                purchase_quantity, purchase_quantity_source, required_card_count,
                quantity_confirmed_at, status, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 0, 'pending', ?, ?)
            ON CONFLICT(uid, order_id) DO NOTHING
            "#,
        )
        .bind(&input.uid)
        .bind(input.rule_id)
        .bind(&input.order_id)
        .bind(&input.buyer_id)
        .bind(&input.item_id)
        .bind(&snapshot.pool_owner_key)
        .bind(&snapshot.pool_scope_snapshot)
        .bind(serialize_string_vec(&snapshot.one_time_pool_tags)?)
        .bind(serialize_string_vec(&snapshot.fixed_pool_tags)?)
        .bind(snapshot.fixed_message_snapshot.trim())
        .bind(input.purchase_quantity)
        .bind(input.purchase_quantity_source.trim())
        .bind(input.required_card_count)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query(
            r#"
            SELECT
                id, uid, rule_id, order_id, buyer_id, item_id, pool_owner_key,
                pool_scope_snapshot, one_time_pool_tags_json, fixed_pool_tags_json,
                fixed_message_snapshot, fixed_message_snapshot_captured, purchase_quantity,
                purchase_quantity_source, required_card_count, quantity_confirmed_at, status, attempt_count, waiting_notice_sent_at,
                waiting_notice_claimed_at, waiting_notice_claim_owner, waiting_notice_claim_expires_at,
                admin_notified_at, consigned_at, message_sent_at, delivery_send_claimed_at,
                delivery_send_claim_owner, delivery_send_claim_expires_at, delivery_send_started_at,
                lease_owner, lease_expires_at, message_acknowledged_at, message_ack_id,
                message_ack_uuid, completed_at, cancel_observed_at, next_retry_at,
                last_error_code, last_error_message, created_at, updated_at
            FROM delivery_runs
            WHERE uid = ? AND order_id = ?
            "#,
        )
        .bind(&input.uid)
        .bind(&input.order_id)
        .fetch_one(&mut *tx)
        .await?;
        let run = delivery_run_from_row(row)?;
        maybe_backfill_run_snapshot(self, &mut tx, &run).await?;
        let row = load_delivery_run_row(&mut *tx, run.id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: format!("delivery run {} disappeared while persisting", run.id),
            })?;
        tx.commit().await?;
        delivery_run_from_row(row)
    }

    pub async fn delivery_run(&self, run_id: i64) -> Result<Option<DeliveryRun>, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "delivery_run").await?;
        let Some(row) = load_delivery_run_row(&mut *tx, run_id).await? else {
            tx.commit().await?;
            return Ok(None);
        };
        let run = delivery_run_from_row(row)?;
        maybe_backfill_run_snapshot(self, &mut tx, &run).await?;
        let row = load_delivery_run_row(&mut *tx, run_id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: format!("delivery run {run_id} disappeared"),
            })?;
        tx.commit().await?;
        Ok(Some(delivery_run_from_row(row)?))
    }

    pub async fn delivery_run_raw(&self, run_id: i64) -> Result<Option<DeliveryRun>, StorageError> {
        let Some(row) = load_delivery_run_row(&self.accounts, run_id).await? else {
            return Ok(None);
        };
        Ok(Some(delivery_run_from_row(row)?))
    }

    pub async fn create_delivery_runs_for_ready_orders(
        &self,
        uid: &str,
    ) -> Result<i64, StorageError> {
        let uid = uid.trim();
        if uid.is_empty() {
            return Ok(0);
        }
        let configurations = self.list_delivery_configuration_records(uid).await?;
        let mut created = 0_i64;
        for configuration in configurations
            .into_iter()
            .filter(|record| record.enabled && !record.needs_repair)
        {
            let orders = sqlx::query(
                r#"
                SELECT tid, buyer_id, item_id, purchase_quantity, status, status_text
                     , COALESCE(purchase_quantity_source, '') AS purchase_quantity_source
                FROM account_orders
                WHERE uid = ? AND item_id = ?
                ORDER BY updated_at DESC, created_at DESC, tid DESC
                LIMIT 200
                "#,
            )
            .bind(uid)
            .bind(&configuration.item_id)
            .fetch_all(&self.xianyu)
            .await?;

            for order in orders {
                let status = order.get::<String, _>("status");
                let status_text = order.get::<String, _>("status_text");
                if !is_ready_to_auto_delivery(&status, &status_text) {
                    continue;
                }
                let buyer_id = order.get::<String, _>("buyer_id");
                if buyer_id.trim().is_empty() {
                    continue;
                }
                let order_id = order.get::<String, _>("tid");
                let existed = self
                    .delivery_run_id_by_uid_order(uid, &order_id)
                    .await?
                    .is_some();
                let purchase_quantity = order.get::<Option<i64>, _>("purchase_quantity");
                let required_card_count = if configuration.one_time_enabled {
                    purchase_quantity.unwrap_or(0)
                } else {
                    0
                };
                self.ensure_delivery_run(NewDeliveryRun {
                    uid: uid.to_string(),
                    rule_id: configuration.id,
                    order_id,
                    buyer_id,
                    item_id: order.get::<String, _>("item_id"),
                    purchase_quantity,
                    purchase_quantity_source: order.get::<String, _>("purchase_quantity_source"),
                    required_card_count,
                })
                .await?;
                if !existed {
                    created += 1;
                }
            }
        }
        Ok(created)
    }

    /// Ready-to-ship orders that cannot enter the delivery queue because their item has no
    /// enabled delivery rule.  They used to be silently skipped by the rule-oriented scan.
    pub async fn pending_delivery_order_mismatches(
        &self,
        uid: &str,
    ) -> Result<Vec<PendingDeliveryOrderMismatch>, StorageError> {
        let uid = uid.trim();
        if uid.is_empty() {
            return Ok(Vec::new());
        }
        // `account_orders` lives in xianyu.db while delivery rules live in
        // accounts.db. SQLite pools do not share attached schemas, so resolve
        // enabled rule item IDs from the accounts pool before querying orders.
        let configured_item_ids = self
            .list_delivery_configuration_records(uid)
            .await?
            .into_iter()
            .filter(|record| record.enabled && !record.needs_repair)
            .map(|record| record.item_id)
            .collect::<std::collections::HashSet<_>>();
        let rows = sqlx::query(
            r#"
            SELECT tid, item_id, COALESCE(title, '') AS title, status, status_text
            FROM account_orders
            WHERE uid = ?
              AND COALESCE(item_id, '') <> ''
            ORDER BY updated_at DESC, created_at DESC, tid DESC
            LIMIT 200
            "#,
        )
        .bind(uid)
        .fetch_all(&self.xianyu)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let item_id = row.get::<String, _>("item_id");
                let status = row.get::<String, _>("status");
                let status_text = row.get::<String, _>("status_text");
                (!configured_item_ids.contains(&item_id)
                    && is_ready_to_auto_delivery(&status, &status_text))
                .then(|| PendingDeliveryOrderMismatch {
                    order_id: row.get("tid"),
                    item_id,
                    title: row.get("title"),
                    status,
                    status_text,
                })
            })
            .collect())
    }

    /// Persist orders that matched an enabled rule but were first observed after they
    /// had already closed. They cannot be safely delivered retrospectively, but must
    /// never disappear without an operator-visible audit trail.
    pub async fn reconcile_unqueued_closed_orders(&self, uid: &str) -> Result<i64, StorageError> {
        let uid = uid.trim();
        if uid.is_empty() {
            return Ok(0);
        }
        let configurations = self.list_delivery_configuration_records(uid).await?;
        let mut recorded = 0_i64;
        for configuration in configurations
            .into_iter()
            .filter(|record| record.enabled && !record.needs_repair)
        {
            let orders = sqlx::query(
                r#"
                SELECT tid, item_id, status, status_text
                FROM account_orders
                WHERE uid = ? AND item_id = ?
                ORDER BY updated_at DESC, created_at DESC, tid DESC
                LIMIT 200
                "#,
            )
            .bind(uid)
            .bind(&configuration.item_id)
            .fetch_all(&self.xianyu)
            .await?;

            for order in orders {
                let status = order.get::<String, _>("status");
                let status_text = order.get::<String, _>("status_text");
                if !is_closed_or_refunded_order(&status, &status_text) {
                    continue;
                }
                let order_id = order.get::<String, _>("tid");
                let run_exists = self
                    .delivery_run_id_by_uid_order(uid, &order_id)
                    .await?
                    .is_some();
                if run_exists {
                    self.resolve_delivery_discovery_alert(&format!("missed_run:{uid}:{order_id}"))
                        .await?;
                    continue;
                }
                if self
                    .upsert_delivery_discovery_alert(
                    &format!("missed_run:{uid}:{order_id}"),
                    "missed_delivery_run",
                    uid,
                    Some(configuration.id),
                    &order_id,
                    &order.get::<String, _>("item_id"),
                    &status,
                    &status_text,
                    "疑似自动发货漏单",
                    &format!(
                        "订单 {order_id} 匹配自动发货配置，但首次核对时已处于“{status_text}”且没有自动发货任务；请人工核对是否漏发。"
                    ),
                    )
                    .await?
                {
                    recorded += 1;
                }
            }
        }
        Ok(recorded)
    }

    pub async fn upsert_delivery_sync_failure_alert(
        &self,
        uid: &str,
        reason: &str,
    ) -> Result<(), StorageError> {
        self.upsert_delivery_discovery_alert(
            &format!("sync_failure:{}", uid.trim()),
            "order_sync_failure",
            uid,
            None,
            "",
            "",
            "",
            "",
            "自动发货订单同步失败",
            &format!(
                "自动发货刷新订单失败：{}。本轮将仅检查本地缓存，可能遗漏新订单。",
                reason.trim()
            ),
        )
        .await
        .map(|_| ())
    }

    pub async fn resolve_delivery_sync_failure_alert(&self, uid: &str) -> Result<(), StorageError> {
        self.resolve_delivery_discovery_alert(&format!("sync_failure:{}", uid.trim()))
            .await
    }

    pub async fn list_delivery_discovery_alerts(
        &self,
        uids: &[String],
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<DeliveryDiscoveryAlert>, StorageError> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id, alert_key, alert_type, uid, rule_id, order_id, item_id, order_status, order_status_text, status, title, body, occurrence_count, first_seen_at, last_seen_at, resolved_at, created_at, updated_at FROM delivery_discovery_alerts WHERE uid IN (",
        );
        let mut separated = builder.separated(", ");
        for uid in uids {
            separated.push_bind(uid);
        }
        separated.push_unseparated(")");
        if let Some(status) = status.filter(|value| !value.trim().is_empty()) {
            builder.push(" AND status = ").push_bind(status.trim());
        }
        builder
            .push(" ORDER BY updated_at DESC, id DESC LIMIT ")
            .push_bind(page_size.clamp(1, 200))
            .push(" OFFSET ")
            .push_bind((page.saturating_sub(1)).saturating_mul(page_size.clamp(1, 200)));
        let rows = builder.build().fetch_all(&self.accounts).await?;
        Ok(rows
            .into_iter()
            .map(delivery_discovery_alert_from_row)
            .collect())
    }

    async fn upsert_delivery_discovery_alert(
        &self,
        alert_key: &str,
        alert_type: &str,
        uid: &str,
        rule_id: Option<i64>,
        order_id: &str,
        item_id: &str,
        order_status: &str,
        order_status_text: &str,
        title: &str,
        body: &str,
    ) -> Result<bool, StorageError> {
        let now = unix_millis_now();
        let was_open = sqlx::query_scalar::<_, String>(
            "SELECT status FROM delivery_discovery_alerts WHERE alert_key = ?",
        )
        .bind(alert_key)
        .fetch_optional(&self.accounts)
        .await?
        .map(|status| status == "open")
        .unwrap_or(false);
        sqlx::query(
            r#"
            INSERT INTO delivery_discovery_alerts (
                alert_key, alert_type, uid, rule_id, order_id, item_id, order_status,
                order_status_text, status, title, body, first_seen_at, last_seen_at, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'open', ?, ?, ?, ?, ?, ?)
            ON CONFLICT(alert_key) DO UPDATE SET
                alert_type = excluded.alert_type,
                uid = excluded.uid,
                rule_id = excluded.rule_id,
                item_id = excluded.item_id,
                order_status = excluded.order_status,
                order_status_text = excluded.order_status_text,
                status = 'open',
                title = excluded.title,
                body = excluded.body,
                occurrence_count = delivery_discovery_alerts.occurrence_count + 1,
                last_seen_at = excluded.last_seen_at,
                resolved_at = 0,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(alert_key)
        .bind(alert_type)
        .bind(uid.trim())
        .bind(rule_id)
        .bind(order_id.trim())
        .bind(item_id.trim())
        .bind(order_status.trim())
        .bind(order_status_text.trim())
        .bind(title)
        .bind(body)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&self.accounts)
        .await?;
        Ok(!was_open)
    }

    async fn resolve_delivery_discovery_alert(&self, alert_key: &str) -> Result<(), StorageError> {
        let now = unix_millis_now();
        sqlx::query(
            "UPDATE delivery_discovery_alerts SET status = 'resolved', resolved_at = ?, updated_at = ? WHERE alert_key = ? AND status != 'resolved'",
        )
        .bind(now)
        .bind(now)
        .bind(alert_key)
        .execute(&self.accounts)
        .await?;
        Ok(())
    }

    async fn delivery_run_id_by_uid_order(
        &self,
        uid: &str,
        order_id: &str,
    ) -> Result<Option<i64>, StorageError> {
        let id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM delivery_runs WHERE uid = ? AND order_id = ? LIMIT 1",
        )
        .bind(uid.trim())
        .bind(order_id.trim())
        .fetch_optional(&self.accounts)
        .await?;
        Ok(id)
    }

    pub async fn list_due_delivery_run_ids(
        &self,
        uid: &str,
        limit: i64,
    ) -> Result<Vec<i64>, StorageError> {
        let limit = limit.clamp(1, 200);
        let now = unix_millis_now();
        let rows = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT id
            FROM delivery_runs
            WHERE uid = ?
              AND status IN (
                'pending', 'waiting_stock', 'reserved',
                'consigning', 'sending', 'retrying'
              )
              AND next_retry_at <= ?
            ORDER BY updated_at ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(uid.trim())
        .bind(now)
        .bind(limit)
        .fetch_all(&self.accounts)
        .await?;
        Ok(rows)
    }

    pub async fn claim_due_delivery_runs(
        &self,
        uid: &str,
        owner: &str,
        now: i64,
        ttl_ms: i64,
        limit: i64,
    ) -> Result<Vec<i64>, StorageError> {
        let owner = owner.trim();
        if owner.is_empty() {
            return Err(StorageError::Conflict {
                message: "delivery run lease owner cannot be empty".to_string(),
            });
        }
        let expires_at = now.saturating_add(ttl_ms.max(1));
        let mut tx = begin_immediate_with_retry(&self.accounts, "claim_due_delivery_runs").await?;
        let candidates = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT id FROM delivery_runs
            WHERE uid = ?
              AND status IN ('pending', 'waiting_stock', 'reserved', 'consigning', 'sending', 'retrying')
              AND next_retry_at <= ?
              AND (lease_expires_at <= ? OR lease_owner = ?)
            ORDER BY updated_at ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(uid.trim())
        .bind(now)
        .bind(now)
        .bind(owner)
        .bind(limit.clamp(1, 200))
        .fetch_all(&mut *tx)
        .await?;
        let mut claimed = Vec::with_capacity(candidates.len());
        for run_id in candidates {
            let result = sqlx::query(
                r#"
                UPDATE delivery_runs
                SET lease_owner = ?, lease_expires_at = ?, updated_at = ?
                WHERE id = ?
                  AND status IN ('pending', 'waiting_stock', 'reserved', 'consigning', 'sending', 'retrying')
                  AND next_retry_at <= ?
                  AND (lease_expires_at <= ? OR lease_owner = ?)
                "#,
            )
            .bind(owner)
            .bind(expires_at)
            .bind(now)
            .bind(run_id)
            .bind(now)
            .bind(now)
            .bind(owner)
            .execute(&mut *tx)
            .await?;
            if result.rows_affected() == 1 {
                claimed.push(run_id);
            }
        }
        tx.commit().await?;
        Ok(claimed)
    }

    pub async fn renew_delivery_run_lease(
        &self,
        run_id: i64,
        owner: &str,
        now: i64,
        ttl_ms: i64,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE delivery_runs SET lease_expires_at = ?, updated_at = ?
            WHERE id = ? AND lease_owner = ?
              AND status NOT IN ('completed', 'cancelled', 'manual_required')
            "#,
        )
        .bind(now.saturating_add(ttl_ms.max(1)))
        .bind(now)
        .bind(run_id)
        .bind(owner.trim())
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn delivery_run_cards(
        &self,
        run_id: i64,
    ) -> Result<Vec<DeliveryRunCard>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT run_id, card_id, position, content_snapshot, status, created_at, updated_at
            FROM delivery_run_cards
            WHERE run_id = ?
            ORDER BY position ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.accounts)
        .await?;
        Ok(rows.into_iter().map(delivery_run_card_from_row).collect())
    }

    pub async fn transition_delivery_run(
        &self,
        run_id: i64,
        expected: DeliveryRunStatus,
        next: DeliveryRunStatus,
        error_code: &str,
        error_message: &str,
        next_retry_at: i64,
        increment_attempt: bool,
    ) -> Result<bool, StorageError> {
        let now = unix_millis_now();
        let result = sqlx::query(
            r#"
            UPDATE delivery_runs
            SET status = ?,
                attempt_count = attempt_count + ?,
                next_retry_at = ?,
                last_error_code = ?,
                last_error_message = ?,
                admin_notified_at = CASE
                    WHEN ? = 'waiting_stock' AND admin_notified_at = 0 THEN ?
                    ELSE admin_notified_at
                END,
                consigned_at = CASE
                    WHEN ? = 'sending' AND consigned_at = 0 THEN ?
                    ELSE consigned_at
                END,
                updated_at = ?
            WHERE id = ? AND status = ?
            "#,
        )
        .bind(next.as_str())
        .bind(if increment_attempt { 1_i64 } else { 0_i64 })
        .bind(next_retry_at)
        .bind(error_code.trim())
        .bind(error_message.trim())
        .bind(next.as_str())
        .bind(now)
        .bind(next.as_str())
        .bind(now)
        .bind(now)
        .bind(run_id)
        .bind(expected.as_str())
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn claim_waiting_notice(
        &self,
        run_id: i64,
        owner: &str,
        now: i64,
        ttl_ms: i64,
    ) -> Result<SideEffectClaim, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "claim_waiting_notice").await?;
        let row = sqlx::query(
            "SELECT status, waiting_notice_sent_at, waiting_notice_claimed_at, waiting_notice_claim_owner, waiting_notice_claim_expires_at FROM delivery_runs WHERE id = ?",
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        if row.get::<i64, _>("waiting_notice_sent_at") > 0 {
            tx.commit().await?;
            return Ok(SideEffectClaim::AlreadyAcknowledged);
        }
        let claimed_at = row.get::<i64, _>("waiting_notice_claimed_at");
        let claim_owner = row.get::<String, _>("waiting_notice_claim_owner");
        let claim_expires_at = row.get::<i64, _>("waiting_notice_claim_expires_at");
        if claimed_at > 0 {
            tx.commit().await?;
            return Ok(if claim_expires_at <= now {
                SideEffectClaim::ExpiredUncertain
            } else if claim_owner == owner.trim() {
                SideEffectClaim::Acquired
            } else {
                SideEffectClaim::ActiveElsewhere
            });
        }
        if row.get::<String, _>("status") != "waiting_stock" {
            tx.commit().await?;
            return Ok(SideEffectClaim::ActiveElsewhere);
        }
        let result = sqlx::query(
            r#"
            UPDATE delivery_runs SET waiting_notice_claimed_at = ?,
                waiting_notice_claim_owner = ?, waiting_notice_claim_expires_at = ?, updated_at = ?
            WHERE id = ? AND status = 'waiting_stock'
              AND waiting_notice_sent_at = 0 AND waiting_notice_claimed_at = 0
            "#,
        )
        .bind(now.max(1))
        .bind(owner.trim())
        .bind(now.saturating_add(ttl_ms.max(1)))
        .bind(now)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        let outcome = if result.rows_affected() == 1 {
            SideEffectClaim::Acquired
        } else {
            SideEffectClaim::ActiveElsewhere
        };
        tx.commit().await?;
        Ok(outcome)
    }

    pub async fn finish_waiting_notice(
        &self,
        run_id: i64,
        owner: &str,
    ) -> Result<bool, StorageError> {
        let now = unix_millis_now();
        let result = sqlx::query(
            r#"
            UPDATE delivery_runs SET waiting_notice_sent_at = ?, waiting_notice_claimed_at = 0,
                waiting_notice_claim_owner = '', waiting_notice_claim_expires_at = 0, updated_at = ?
            WHERE id = ? AND waiting_notice_sent_at = 0 AND waiting_notice_claim_owner = ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(run_id)
        .bind(owner.trim())
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn reset_waiting_notice_claim(
        &self,
        run_id: i64,
        owner: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE delivery_runs SET waiting_notice_claimed_at = 0, waiting_notice_claim_owner = '', waiting_notice_claim_expires_at = 0, updated_at = ? WHERE id = ? AND waiting_notice_claim_owner = ? AND waiting_notice_sent_at = 0",
        )
        .bind(unix_millis_now())
        .bind(run_id)
        .bind(owner.trim())
        .execute(&self.accounts)
        .await?;
        Ok(())
    }

    pub async fn claim_delivery_send(
        &self,
        run_id: i64,
        owner: &str,
        now: i64,
        ttl_ms: i64,
    ) -> Result<SideEffectClaim, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "claim_delivery_send").await?;
        let row = sqlx::query(
            "SELECT status, message_sent_at, message_acknowledged_at, delivery_send_claimed_at, delivery_send_claim_owner, delivery_send_claim_expires_at, delivery_send_started_at FROM delivery_runs WHERE id = ?",
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        if row.get::<i64, _>("message_acknowledged_at") > 0
            || row.get::<i64, _>("message_sent_at") > 0
        {
            tx.commit().await?;
            return Ok(SideEffectClaim::AlreadyAcknowledged);
        }
        let claimed_at = row.get::<i64, _>("delivery_send_claimed_at");
        let claim_owner = row.get::<String, _>("delivery_send_claim_owner");
        let claim_expires_at = row.get::<i64, _>("delivery_send_claim_expires_at");
        if claimed_at > 0 {
            // Pre-0032 rows only have claimed_at; owner/expiry default empty/0 and must
            // fail closed as uncertain rather than looking like a live foreign claim.
            let legacy_claim = claim_owner.trim().is_empty() || claim_expires_at <= 0;
            let outcome = if legacy_claim || claim_expires_at <= now {
                SideEffectClaim::ExpiredUncertain
            } else if claim_owner == owner.trim() {
                SideEffectClaim::Acquired
            } else {
                SideEffectClaim::ActiveElsewhere
            };
            tx.commit().await?;
            return Ok(outcome);
        }
        if row.get::<String, _>("status") != "sending" {
            tx.commit().await?;
            return Ok(SideEffectClaim::ActiveElsewhere);
        }
        let result = sqlx::query(
            r#"
            UPDATE delivery_runs SET delivery_send_claimed_at = ?, delivery_send_claim_owner = ?,
                delivery_send_claim_expires_at = ?, delivery_send_started_at = ?, updated_at = ?
            WHERE id = ? AND status = 'sending'
              AND message_sent_at = 0 AND delivery_send_claimed_at = 0
            "#,
        )
        .bind(now.max(1))
        .bind(owner.trim())
        .bind(now.saturating_add(ttl_ms.max(1)))
        .bind(now.max(1))
        .bind(now)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        let outcome = if result.rows_affected() == 1 {
            SideEffectClaim::Acquired
        } else {
            SideEffectClaim::ActiveElsewhere
        };
        tx.commit().await?;
        Ok(outcome)
    }

    pub async fn clear_delivery_send_claim(&self, run_id: i64) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            UPDATE delivery_runs
            SET delivery_send_claimed_at = 0, updated_at = ?
            WHERE id = ? AND message_sent_at = 0 AND delivery_send_claimed_at > 0
            "#,
        )
        .bind(unix_millis_now())
        .bind(run_id)
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn acknowledge_and_complete_delivery_run(
        &self,
        run_id: i64,
        claim_owner: &str,
        order_id: &str,
        message_id: &str,
        message_uuid: &str,
    ) -> Result<DeliveryCompletionOutcome, StorageError> {
        let claim_owner = claim_owner.trim();
        let message_id = message_id.trim();
        let message_uuid = message_uuid.trim();
        if claim_owner.is_empty() || message_id.is_empty() || message_uuid.is_empty() {
            return Err(StorageError::Conflict {
                message: "delivery ACK requires claim owner, message id, and uuid".to_string(),
            });
        }
        let mut tx =
            begin_immediate_with_retry(&self.accounts, "acknowledge_and_complete_delivery_run")
                .await?;
        let row = sqlx::query(
            r#"
            SELECT order_id, required_card_count, status, delivery_send_claimed_at,
                   delivery_send_claim_owner, message_acknowledged_at,
                   message_ack_id, message_ack_uuid
            FROM delivery_runs WHERE id = ?
            "#,
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::Conflict {
            message: format!("delivery run {run_id} does not exist"),
        })?;
        let expected_order_id = row.get::<String, _>("order_id");
        if expected_order_id != order_id {
            return Err(StorageError::Conflict {
                message: format!(
                    "delivery run {run_id} belongs to order {expected_order_id}, not {order_id}"
                ),
            });
        }
        let existing_ack_at = row.get::<i64, _>("message_acknowledged_at");
        let existing_ack_id = row.get::<String, _>("message_ack_id");
        let existing_ack_uuid = row.get::<String, _>("message_ack_uuid");
        if existing_ack_at > 0
            && (existing_ack_id != message_id || existing_ack_uuid != message_uuid)
        {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} already has a different ACK"),
            });
        }
        let now = unix_millis_now().max(1);
        sqlx::query(
            r#"
            UPDATE delivery_runs
            SET message_acknowledged_at = CASE
                    WHEN message_acknowledged_at = 0 THEN ? ELSE message_acknowledged_at END,
                message_ack_id = CASE WHEN message_ack_id = '' THEN ? ELSE message_ack_id END,
                message_ack_uuid = CASE WHEN message_ack_uuid = '' THEN ? ELSE message_ack_uuid END,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now)
        .bind(message_id)
        .bind(message_uuid)
        .bind(now)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        let status = row.get::<String, _>("status");
        if status == "completed" {
            tx.commit().await?;
            return Ok(DeliveryCompletionOutcome::AlreadyCompleted);
        }

        let required = row.get::<i64, _>("required_card_count");
        let cards = load_delivery_run_cards(&mut tx, run_id).await?;
        let mut completion_safe = status == "sending"
            && row.get::<i64, _>("delivery_send_claimed_at") > 0
            && row.get::<String, _>("delivery_send_claim_owner") == claim_owner
            && cards.len() as i64 == required;
        if completion_safe {
            for card in &cards {
                if card.status != "reserved" {
                    completion_safe = false;
                    break;
                }
                let owned = sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT COUNT(*) FROM cards
                    WHERE id = ? AND reserved_run_id = ? AND reserved_order_id = ? AND used = 0
                    "#,
                )
                .bind(card.card_id)
                .bind(run_id)
                .bind(order_id)
                .fetch_one(&mut *tx)
                .await?;
                if owned != 1 {
                    completion_safe = false;
                    break;
                }
            }
        }
        if !completion_safe {
            sqlx::query(
                r#"
                UPDATE delivery_runs
                SET status = 'manual_required', next_retry_at = 0,
                    last_error_code = 'ack_completion_conflict',
                    last_error_message = '消息已确认送达，但交付完成条件发生冲突，请人工核对',
                    updated_at = ?
                WHERE id = ? AND status != 'completed'
                "#,
            )
            .bind(now)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(DeliveryCompletionOutcome::AckPersistedButCompletionConflict);
        }

        let completed = sqlx::query(
            r#"
            UPDATE delivery_runs
            SET status = 'completed', message_sent_at = ?, completed_at = ?,
                delivery_send_claimed_at = 0, delivery_send_claim_owner = '',
                delivery_send_claim_expires_at = 0, next_retry_at = 0,
                last_error_code = '', last_error_message = '', updated_at = ?
            WHERE id = ? AND status = 'sending' AND delivery_send_claim_owner = ?
              AND message_ack_id = ? AND message_ack_uuid = ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(run_id)
        .bind(claim_owner)
        .bind(message_id)
        .bind(message_uuid)
        .execute(&mut *tx)
        .await?;
        if completed.rows_affected() != 1 {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} completion CAS failed while locked"),
            });
        }
        for card in &cards {
            sqlx::query(
                r#"
                UPDATE cards SET used = 1, used_at = ?, used_order_id = ?,
                    reserved_run_id = NULL, reserved_order_id = '', reserved_at = 0
                WHERE id = ? AND reserved_run_id = ? AND reserved_order_id = ? AND used = 0
                "#,
            )
            .bind(now)
            .bind(order_id)
            .bind(card.card_id)
            .bind(run_id)
            .bind(order_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE delivery_run_cards SET status = 'consumed', updated_at = ? WHERE run_id = ? AND card_id = ? AND status = 'reserved'")
                .bind(now).bind(run_id).bind(card.card_id).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE delivery_alerts SET status = 'resolved', resolved_at = ?, updated_at = ? WHERE run_id = ? AND status != 'resolved'")
            .bind(now).bind(now).bind(run_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(DeliveryCompletionOutcome::Completed)
    }
    pub async fn complete_delivery_run(
        &self,
        run_id: i64,
        order_id: &str,
    ) -> Result<bool, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "complete_delivery_run").await?;
        let run_row = sqlx::query(
            "SELECT order_id, required_card_count, status, delivery_send_claimed_at FROM delivery_runs WHERE id = ?",
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::Conflict {
            message: format!("delivery run {run_id} does not exist"),
        })?;
        let expected_order_id = run_row.get::<String, _>("order_id");
        if expected_order_id != order_id {
            return Err(StorageError::Conflict {
                message: format!(
                    "delivery run {run_id} belongs to order {expected_order_id}, not {order_id}"
                ),
            });
        }
        if run_row.get::<String, _>("status") == "completed" {
            tx.commit().await?;
            return Ok(false);
        }
        if run_row.get::<String, _>("status") != "sending" {
            return Ok(false);
        }
        if run_row.get::<i64, _>("delivery_send_claimed_at") <= 0 {
            return Ok(false);
        }

        let now = unix_millis_now();
        let cards = load_delivery_run_cards(&mut tx, run_id).await?;
        let required = run_row.get::<i64, _>("required_card_count");
        if cards.len() as i64 != required {
            return Err(StorageError::Conflict {
                message: format!(
                    "delivery run {run_id} requires {required} cards but maps {}",
                    cards.len()
                ),
            });
        }
        for card in cards.iter().filter(|card| card.status != "consumed") {
            let updated = sqlx::query(
                r#"
                UPDATE cards
                SET used = 1, used_at = ?, used_order_id = ?,
                    reserved_run_id = NULL, reserved_order_id = '', reserved_at = 0
                WHERE id = ? AND reserved_run_id = ? AND reserved_order_id = ? AND used = 0
                "#,
            )
            .bind(now)
            .bind(order_id)
            .bind(card.card_id)
            .bind(run_id)
            .bind(order_id)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StorageError::Conflict {
                    message: format!(
                        "card {} is no longer reserved by delivery run {run_id}",
                        card.card_id
                    ),
                });
            }
            sqlx::query(
                "UPDATE delivery_run_cards SET status = 'consumed', updated_at = ? WHERE run_id = ? AND card_id = ? AND status = 'reserved'",
            )
            .bind(now)
            .bind(run_id)
            .bind(card.card_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "UPDATE delivery_alerts SET status = 'resolved', resolved_at = ?, updated_at = ? WHERE run_id = ? AND status != 'resolved'",
        )
        .bind(now)
        .bind(now)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        let completed = sqlx::query(
            r#"
            UPDATE delivery_runs
            SET status = 'completed', message_sent_at = ?, completed_at = ?,
                delivery_send_claimed_at = 0, next_retry_at = 0,
                last_error_code = '', last_error_message = '', updated_at = ?
            WHERE id = ? AND status = 'sending' AND delivery_send_claimed_at > 0
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        if completed.rows_affected() != 1 {
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }

    pub async fn cancel_delivery_run(
        &self,
        run_id: i64,
    ) -> Result<CancelDeliveryRunOutcome, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "cancel_delivery_run").await?;
        let now = unix_millis_now();
        let row = sqlx::query(
            r#"
            SELECT status, waiting_notice_sent_at, waiting_notice_claimed_at,
                   consigned_at, delivery_send_claimed_at, delivery_send_started_at,
                   message_sent_at, message_acknowledged_at
            FROM delivery_runs WHERE id = ?
            "#,
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        let status = row.get::<String, _>("status");
        if matches!(status.as_str(), "completed" | "cancelled") {
            tx.commit().await?;
            return Ok(CancelDeliveryRunOutcome::AlreadyTerminal);
        }
        let side_effect_observed = matches!(status.as_str(), "consigning" | "sending")
            || row.get::<i64, _>("waiting_notice_sent_at") > 0
            || row.get::<i64, _>("waiting_notice_claimed_at") > 0
            || row.get::<i64, _>("consigned_at") > 0
            || row.get::<i64, _>("delivery_send_claimed_at") > 0
            || row.get::<i64, _>("delivery_send_started_at") > 0
            || row.get::<i64, _>("message_sent_at") > 0
            || row.get::<i64, _>("message_acknowledged_at") > 0;
        let safely_cancellable = matches!(
            status.as_str(),
            "pending" | "waiting_stock" | "reserved" | "retrying"
        ) && !side_effect_observed;
        if !safely_cancellable {
            sqlx::query(
                r#"
                UPDATE delivery_runs
                SET cancel_observed_at = CASE WHEN cancel_observed_at = 0 THEN ? ELSE cancel_observed_at END,
                    last_error_code = 'order_cancel_observed_in_flight',
                    last_error_message = '订单取消已观察到，但副作用可能正在执行，保留现场等待收敛',
                    updated_at = ?
                WHERE id = ?
                "#,
            )
            .bind(now)
            .bind(now)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(CancelDeliveryRunOutcome::ObservedInFlight);
        }
        let cancelled = sqlx::query(
            r#"
            UPDATE delivery_runs
            SET status = 'cancelled', next_retry_at = 0,
                last_error_code = 'order_cancelled', last_error_message = '订单已取消或退款',
                cancel_observed_at = CASE WHEN cancel_observed_at = 0 THEN ? ELSE cancel_observed_at END,
                updated_at = ?
            WHERE id = ? AND status = ?
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(run_id)
        .bind(&status)
        .execute(&mut *tx)
        .await?;
        if cancelled.rows_affected() == 0 {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} changed while cancelling"),
            });
        }
        sqlx::query(
            r#"
            UPDATE cards
            SET reserved_run_id = NULL, reserved_order_id = '', reserved_at = 0
            WHERE reserved_run_id = ? AND used = 0
            "#,
        )
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM delivery_run_cards WHERE run_id = ? AND status = 'reserved'")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(CancelDeliveryRunOutcome::CancelledAndReleased)
    }

    /// Seller (or another path) already shipped the order. Drop auto-delivery without
    /// sending fixed text / cards. Safe only when this pipeline has not started side effects.
    pub async fn skip_delivery_run_external_ship(
        &self,
        run_id: i64,
        reason: &str,
    ) -> Result<CancelDeliveryRunOutcome, StorageError> {
        let mut tx =
            begin_immediate_with_retry(&self.accounts, "skip_delivery_run_external_ship").await?;
        let now = unix_millis_now();
        let row = sqlx::query(
            r#"
            SELECT status, waiting_notice_sent_at, waiting_notice_claimed_at,
                   consigned_at, delivery_send_claimed_at, delivery_send_started_at,
                   message_sent_at, message_acknowledged_at
            FROM delivery_runs WHERE id = ?
            "#,
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        let status = row.get::<String, _>("status");
        if matches!(status.as_str(), "completed" | "cancelled") {
            tx.commit().await?;
            return Ok(CancelDeliveryRunOutcome::AlreadyTerminal);
        }
        // If we already sent content or completed consign ourselves, keep the run for audit
        // and mark manual required instead of silent cancel.
        let content_or_own_consign = row.get::<i64, _>("message_sent_at") > 0
            || row.get::<i64, _>("message_acknowledged_at") > 0
            || row.get::<i64, _>("consigned_at") > 0
            || row.get::<i64, _>("delivery_send_started_at") > 0
            || row.get::<i64, _>("delivery_send_claimed_at") > 0;
        if content_or_own_consign {
            sqlx::query(
                r#"
                UPDATE delivery_runs
                SET status = 'manual_required',
                    next_retry_at = 0,
                    last_error_code = 'external_ship_after_pipeline_started',
                    last_error_message = ?,
                    updated_at = ?
                WHERE id = ?
                "#,
            )
            .bind(reason.trim())
            .bind(now)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(CancelDeliveryRunOutcome::ObservedInFlight);
        }

        let reason = if reason.trim().is_empty() {
            "订单已手动发货，跳过自动发货"
        } else {
            reason.trim()
        };
        let cancelled = sqlx::query(
            r#"
            UPDATE delivery_runs
            SET status = 'cancelled', next_retry_at = 0,
                last_error_code = 'external_already_shipped',
                last_error_message = ?,
                updated_at = ?
            WHERE id = ? AND status = ?
            "#,
        )
        .bind(reason)
        .bind(now)
        .bind(run_id)
        .bind(&status)
        .execute(&mut *tx)
        .await?;
        if cancelled.rows_affected() == 0 {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} changed while skipping external ship"),
            });
        }
        sqlx::query(
            r#"
            UPDATE cards
            SET reserved_run_id = NULL, reserved_order_id = '', reserved_at = 0
            WHERE reserved_run_id = ? AND used = 0
            "#,
        )
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM delivery_run_cards WHERE run_id = ? AND status = 'reserved'")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(CancelDeliveryRunOutcome::CancelledAndReleased)
    }

    pub async fn confirm_run_quantity_before_reservation(
        &self,
        run_id: i64,
        purchase_quantity: Option<i64>,
        purchase_quantity_source: &str,
        refreshed_at: i64,
    ) -> Result<DeliveryRun, StorageError> {
        let mut tx =
            begin_immediate_with_retry(&self.accounts, "confirm_run_quantity_before_reservation")
                .await?;
        let row = load_delivery_run_row(&mut *tx, run_id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: format!("delivery run {run_id} does not exist"),
            })?;
        let run = delivery_run_from_row(row)?;
        maybe_backfill_run_snapshot(self, &mut tx, &run).await?;
        let row = load_delivery_run_row(&mut *tx, run_id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: format!("delivery run {run_id} disappeared"),
            })?;
        let run = delivery_run_from_row(row)?;
        let reserved_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM delivery_run_cards WHERE run_id = ?",
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        if reserved_count > 0 || run.consigned_at > 0 || run.message_sent_at > 0 {
            tx.commit().await?;
            return Ok(run);
        }

        let has_one_time_cards = !run.one_time_pool_tags.is_empty();
        let normalized_source = purchase_quantity_source.trim();
        let now = refreshed_at.max(1);
        if has_one_time_cards {
            let Some(quantity) = purchase_quantity.filter(|value| *value > 0) else {
                let next_retry_at = quantity_retry_at(run.attempt_count);
                sqlx::query(
                    r#"
                    UPDATE delivery_runs
                    SET purchase_quantity = ?,
                        purchase_quantity_source = ?,
                        required_card_count = 0,
                        quantity_confirmed_at = ?,
                        status = 'retrying',
                        next_retry_at = ?,
                        last_error_code = 'quantity_unknown_retryable',
                        last_error_message = '订单购买数量暂时无法确认，系统将稍后重试',
                        updated_at = ?
                    WHERE id = ?
                    "#,
                )
                .bind(purchase_quantity)
                .bind(normalized_source)
                .bind(now)
                .bind(next_retry_at)
                .bind(unix_millis_now())
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
                let row = load_delivery_run_row(&mut *tx, run_id)
                    .await?
                    .ok_or_else(|| StorageError::Conflict {
                        message: format!("delivery run {run_id} disappeared after quantity retry"),
                    })?;
                tx.commit().await?;
                return delivery_run_from_row(row);
            };
            sqlx::query(
                r#"
                UPDATE delivery_runs
                SET purchase_quantity = ?,
                    purchase_quantity_source = ?,
                    required_card_count = ?,
                    quantity_confirmed_at = ?,
                    status = CASE
                        WHEN status = 'retrying' AND last_error_code = 'quantity_unknown_retryable'
                            THEN 'pending'
                        ELSE status
                    END,
                    next_retry_at = CASE
                        WHEN status = 'retrying' AND last_error_code = 'quantity_unknown_retryable'
                            THEN 0
                        ELSE next_retry_at
                    END,
                    last_error_code = CASE
                        WHEN last_error_code = 'quantity_unknown_retryable' THEN ''
                        ELSE last_error_code
                    END,
                    last_error_message = CASE
                        WHEN last_error_code = 'quantity_unknown_retryable' THEN ''
                        ELSE last_error_message
                    END,
                    updated_at = ?
                WHERE id = ?
                "#,
            )
            .bind(quantity)
            .bind(normalized_source)
            .bind(quantity)
            .bind(now)
            .bind(unix_millis_now())
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE delivery_runs
                SET purchase_quantity = ?,
                    purchase_quantity_source = ?,
                    required_card_count = 0,
                    quantity_confirmed_at = ?,
                    status = CASE
                        WHEN status = 'retrying' AND last_error_code = 'quantity_unknown_retryable'
                            THEN 'pending'
                        ELSE status
                    END,
                    next_retry_at = CASE
                        WHEN status = 'retrying' AND last_error_code = 'quantity_unknown_retryable'
                            THEN 0
                        ELSE next_retry_at
                    END,
                    last_error_code = CASE
                        WHEN last_error_code = 'quantity_unknown_retryable' THEN ''
                        ELSE last_error_code
                    END,
                    last_error_message = CASE
                        WHEN last_error_code = 'quantity_unknown_retryable' THEN ''
                        ELSE last_error_message
                    END,
                    updated_at = ?
                WHERE id = ?
                "#,
            )
            .bind(purchase_quantity)
            .bind(normalized_source)
            .bind(now)
            .bind(unix_millis_now())
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        }
        let row = load_delivery_run_row(&mut *tx, run_id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: format!("delivery run {run_id} disappeared after quantity confirm"),
            })?;
        tx.commit().await?;
        delivery_run_from_row(row)
    }

    pub async fn reserve_cards_for_run(
        &self,
        run_id: i64,
        uid: &str,
        pool_scope: &str,
        card_pool_tag: &str,
        required: i64,
    ) -> Result<CardReservation, StorageError> {
        let Some(run) = self.delivery_run(run_id).await? else {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} does not exist"),
            });
        };
        validate_explicit_reservation_against_snapshot(
            &run,
            uid,
            pool_scope,
            &[card_pool_tag.to_string()],
            required,
        )?;
        self.reserve_cards_for_run_snapshot(run_id).await
    }

    pub async fn reserve_cards_for_run_from_pools(
        &self,
        run_id: i64,
        uid: &str,
        pool_scope: &str,
        card_pool_tags: &[String],
        required: i64,
    ) -> Result<CardReservation, StorageError> {
        let Some(run) = self.delivery_run(run_id).await? else {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} does not exist"),
            });
        };
        validate_explicit_reservation_against_snapshot(
            &run,
            uid,
            pool_scope,
            card_pool_tags,
            required,
        )?;
        self.reserve_cards_for_run_snapshot(run_id).await
    }

    pub async fn reserve_cards_for_run_snapshot(
        &self,
        run_id: i64,
    ) -> Result<CardReservation, StorageError> {
        let mut tx =
            begin_immediate_with_retry(&self.accounts, "reserve_cards_for_run_snapshot").await?;
        let Some(row) = load_delivery_run_row(&mut *tx, run_id).await? else {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} does not exist"),
            });
        };
        let run = delivery_run_from_row(row)?;
        maybe_backfill_run_snapshot(self, &mut tx, &run).await?;
        let row = load_delivery_run_row(&mut *tx, run_id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: format!("delivery run {run_id} disappeared"),
            })?;
        let run = delivery_run_from_row(row)?;
        let order_id = run.order_id.clone();
        let required = run.required_card_count;
        let card_pool_tags = run.one_time_pool_tags.clone();
        if required < 0 {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} cannot reserve a negative card quantity"),
            });
        }

        let existing = load_delivery_run_cards(&mut tx, run_id).await?;
        if !existing.is_empty() {
            if existing.len() as i64 != required {
                return Err(StorageError::Conflict {
                    message: format!(
                        "delivery run {run_id} already maps {} cards but {required} were requested",
                        existing.len()
                    ),
                });
            }
            mark_run_reserved(&mut tx, run_id).await?;
            tx.commit().await?;
            return Ok(CardReservation::Reserved { cards: existing });
        }

        if required == 0 {
            mark_run_reserved(&mut tx, run_id).await?;
            tx.commit().await?;
            return Ok(CardReservation::Reserved { cards: Vec::new() });
        }

        if card_pool_tags.is_empty() {
            tx.rollback().await?;
            return Ok(CardReservation::Insufficient {
                available: 0,
                required,
            });
        }

        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id, card_content, card_pool_tag FROM cards WHERE cookie_id = ",
        );
        builder
            .push_bind(&run.pool_owner_key)
            .push(" AND card_pool_tag IN (");
        let mut separated = builder.separated(", ");
        for tag in card_pool_tags {
            separated.push_bind(tag.clone());
        }
        separated.push_unseparated(")");
        builder
            .push(
                " AND reusable = 0 AND one_time_identity_active = 1 AND used = 0 AND reserved_run_id IS NULL AND TRIM(card_content) != '' ORDER BY created_at ASC, id ASC LIMIT ",
            )
            .push_bind(required);
        let candidates = builder.build().fetch_all(&mut *tx).await?;

        if candidates.len() as i64 != required {
            let available = candidates.len() as i64;
            tx.rollback().await?;
            return Ok(CardReservation::Insufficient {
                available,
                required,
            });
        }

        let now = unix_millis_now();
        let mut cards = Vec::with_capacity(candidates.len());
        for (position, candidate) in candidates.into_iter().enumerate() {
            let card_id = candidate.get::<i64, _>("id");
            let content_snapshot = candidate.get::<String, _>("card_content");
            let candidate_pool_tag = candidate.get::<String, _>("card_pool_tag");
            let updated = sqlx::query(
                r#"
                UPDATE cards
                SET reserved_run_id = ?, reserved_order_id = ?, reserved_at = ?
                WHERE id = ?
                  AND cookie_id = ?
                  AND card_pool_tag = ?
                  AND reusable = 0
                  AND one_time_identity_active = 1
                  AND used = 0
                  AND reserved_run_id IS NULL
                "#,
            )
            .bind(run_id)
            .bind(&order_id)
            .bind(now)
            .bind(card_id)
            .bind(&run.pool_owner_key)
            .bind(&candidate_pool_tag)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StorageError::Conflict {
                    message: format!(
                        "card {card_id} changed while delivery run {run_id} was reserving it"
                    ),
                });
            }

            let position = position as i64;
            sqlx::query(
                r#"
                INSERT INTO delivery_run_cards (
                    run_id, card_id, position, content_snapshot, status, created_at, updated_at
                )
                VALUES (?, ?, ?, ?, 'reserved', ?, ?)
                "#,
            )
            .bind(run_id)
            .bind(card_id)
            .bind(position)
            .bind(&content_snapshot)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            cards.push(DeliveryRunCard {
                run_id,
                card_id,
                position,
                content_snapshot,
                status: "reserved".to_string(),
                created_at: now,
                updated_at: now,
            });
        }

        mark_run_reserved(&mut tx, run_id).await?;
        tx.commit().await?;
        Ok(CardReservation::Reserved { cards })
    }

    async fn reserve_cards_for_run_explicit(
        &self,
        run_id: i64,
        uid: &str,
        pool_scope: &str,
        card_pool_tags: &[String],
        required: i64,
    ) -> Result<CardReservation, StorageError> {
        if required < 0 {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} cannot reserve a negative card quantity"),
            });
        }

        let pool_cookie_id = self.resolve_card_pool_cookie_id(uid, pool_scope).await?;
        let mut tx =
            begin_immediate_with_retry(&self.accounts, "reserve_cards_for_run_explicit").await?;
        let run_row = sqlx::query(
            "SELECT uid, order_id, required_card_count FROM delivery_runs WHERE id = ?",
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(run_row) = run_row else {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} does not exist"),
            });
        };
        let run_uid = run_row.get::<String, _>("uid");
        if run_uid != uid {
            return Err(StorageError::Conflict {
                message: format!("delivery run {run_id} does not belong to account {uid}"),
            });
        }
        let order_id = run_row.get::<String, _>("order_id");
        let required_card_count = run_row.get::<i64, _>("required_card_count");
        if required != required_card_count {
            return Err(StorageError::Conflict {
                message: format!(
                    "delivery run {run_id} requires {required_card_count} cards but {required} were requested"
                ),
            });
        }

        let existing = load_delivery_run_cards(&mut tx, run_id).await?;
        if !existing.is_empty() {
            if existing.len() as i64 != required {
                return Err(StorageError::Conflict {
                    message: format!(
                        "delivery run {run_id} already maps {} cards but {required} were requested",
                        existing.len()
                    ),
                });
            }
            mark_run_reserved(&mut tx, run_id).await?;
            tx.commit().await?;
            return Ok(CardReservation::Reserved { cards: existing });
        }

        if required == 0 {
            mark_run_reserved(&mut tx, run_id).await?;
            tx.commit().await?;
            return Ok(CardReservation::Reserved { cards: Vec::new() });
        }

        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id, card_content, card_pool_tag FROM cards WHERE cookie_id = ",
        );
        builder
            .push_bind(&pool_cookie_id)
            .push(" AND card_pool_tag IN (");
        let mut separated = builder.separated(", ");
        for tag in card_pool_tags {
            separated.push_bind(tag);
        }
        separated.push_unseparated(")");
        builder
            .push(
                " AND reusable = 0 AND one_time_identity_active = 1 AND used = 0 AND reserved_run_id IS NULL AND TRIM(card_content) != '' ORDER BY created_at ASC, id ASC LIMIT ",
            )
            .push_bind(required);
        let candidates = builder.build().fetch_all(&mut *tx).await?;

        if candidates.len() as i64 != required {
            let available = candidates.len() as i64;
            tx.rollback().await?;
            return Ok(CardReservation::Insufficient {
                available,
                required,
            });
        }

        let now = unix_millis_now();
        let mut cards = Vec::with_capacity(candidates.len());
        for (position, candidate) in candidates.into_iter().enumerate() {
            let card_id = candidate.get::<i64, _>("id");
            let content_snapshot = candidate.get::<String, _>("card_content");
            let candidate_pool_tag = candidate.get::<String, _>("card_pool_tag");
            let updated = sqlx::query(
                r#"
                UPDATE cards
                SET reserved_run_id = ?, reserved_order_id = ?, reserved_at = ?
                WHERE id = ?
                  AND cookie_id = ?
                  AND card_pool_tag = ?
                  AND reusable = 0
                  AND one_time_identity_active = 1
                  AND used = 0
                  AND reserved_run_id IS NULL
                "#,
            )
            .bind(run_id)
            .bind(&order_id)
            .bind(now)
            .bind(card_id)
            .bind(&pool_cookie_id)
            .bind(&candidate_pool_tag)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StorageError::Conflict {
                    message: format!(
                        "card {card_id} changed while delivery run {run_id} was reserving it"
                    ),
                });
            }

            let position = position as i64;
            sqlx::query(
                r#"
                INSERT INTO delivery_run_cards (
                    run_id, card_id, position, content_snapshot, status, created_at, updated_at
                )
                VALUES (?, ?, ?, ?, 'reserved', ?, ?)
                "#,
            )
            .bind(run_id)
            .bind(card_id)
            .bind(position)
            .bind(&content_snapshot)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            cards.push(DeliveryRunCard {
                run_id,
                card_id,
                position,
                content_snapshot,
                status: "reserved".to_string(),
                created_at: now,
                updated_at: now,
            });
        }

        mark_run_reserved(&mut tx, run_id).await?;
        tx.commit().await?;
        Ok(CardReservation::Reserved { cards })
    }

    pub async fn consume_reserved_cards(
        &self,
        run_id: i64,
        order_id: &str,
    ) -> Result<Vec<DeliveryRunCard>, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "consume_reserved_cards").await?;
        let run_row =
            sqlx::query("SELECT order_id, required_card_count FROM delivery_runs WHERE id = ?")
                .bind(run_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::Conflict {
                    message: format!("delivery run {run_id} does not exist"),
                })?;
        let expected_order_id = run_row.get::<String, _>("order_id");
        if expected_order_id != order_id {
            return Err(StorageError::Conflict {
                message: format!(
                    "delivery run {run_id} belongs to order {expected_order_id}, not {order_id}"
                ),
            });
        }

        let now = unix_millis_now();
        let mut cards = load_delivery_run_cards(&mut tx, run_id).await?;
        let expected_card_count = run_row.get::<i64, _>("required_card_count");
        if cards.len() as i64 != expected_card_count {
            return Err(StorageError::Conflict {
                message: format!(
                    "delivery run {run_id} requires {expected_card_count} cards but maps {}",
                    cards.len()
                ),
            });
        }
        for card in &mut cards {
            if card.status == "consumed" {
                continue;
            }

            let updated = sqlx::query(
                r#"
                UPDATE cards
                SET used = 1,
                    used_at = ?,
                    used_order_id = ?,
                    reserved_run_id = NULL,
                    reserved_order_id = '',
                    reserved_at = 0
                WHERE id = ?
                  AND reserved_run_id = ?
                  AND reserved_order_id = ?
                  AND used = 0
                "#,
            )
            .bind(now)
            .bind(order_id)
            .bind(card.card_id)
            .bind(run_id)
            .bind(order_id)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StorageError::Conflict {
                    message: format!(
                        "card {} is no longer reserved by delivery run {run_id}",
                        card.card_id
                    ),
                });
            }

            sqlx::query(
                r#"
                UPDATE delivery_run_cards
                SET status = 'consumed', updated_at = ?
                WHERE run_id = ? AND card_id = ? AND status = 'reserved'
                "#,
            )
            .bind(now)
            .bind(run_id)
            .bind(card.card_id)
            .execute(&mut *tx)
            .await?;
            card.status = "consumed".to_string();
            card.updated_at = now;
        }

        tx.commit().await?;
        Ok(cards)
    }

    pub async fn release_reserved_cards(&self, run_id: i64) -> Result<u64, StorageError> {
        let mut tx = begin_immediate_with_retry(&self.accounts, "release_reserved_cards").await?;
        let card_ids = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM cards WHERE reserved_run_id = ? AND used = 0 ORDER BY id",
        )
        .bind(run_id)
        .fetch_all(&mut *tx)
        .await?;
        let mut released = 0_u64;

        for card_id in card_ids {
            let updated = sqlx::query(
                r#"
                UPDATE cards
                SET reserved_run_id = NULL, reserved_order_id = '', reserved_at = 0
                WHERE id = ? AND reserved_run_id = ? AND used = 0
                "#,
            )
            .bind(card_id)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StorageError::Conflict {
                    message: format!(
                        "card {card_id} changed while delivery run {run_id} was releasing it"
                    ),
                });
            }

            sqlx::query("DELETE FROM delivery_run_cards WHERE run_id = ? AND card_id = ?")
                .bind(run_id)
                .bind(card_id)
                .execute(&mut *tx)
                .await?;
            released += 1;
        }

        tx.commit().await?;
        Ok(released)
    }

    pub async fn upsert_waiting_stock_alert(
        &self,
        run: &DeliveryRun,
        title: &str,
        body: &str,
    ) -> Result<DeliveryAlert, StorageError> {
        let now = unix_millis_now();
        sqlx::query(
            r#"
            INSERT INTO delivery_alerts (
                run_id, uid, status, title, body, created_at, updated_at
            )
            VALUES (?, ?, 'open', ?, ?, ?, ?)
            ON CONFLICT(run_id) DO UPDATE SET
                uid = excluded.uid,
                status = 'open',
                title = excluded.title,
                body = excluded.body,
                resolved_at = 0,
                system_notified_at = CASE
                    WHEN delivery_alerts.status = 'open' THEN delivery_alerts.system_notified_at
                    ELSE 0
                END,
                external_notified_at = CASE
                    WHEN delivery_alerts.status = 'open' THEN delivery_alerts.external_notified_at
                    ELSE 0
                END,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(run.id)
        .bind(&run.uid)
        .bind(title)
        .bind(body)
        .bind(now)
        .bind(now)
        .execute(&self.accounts)
        .await?;

        self.delivery_alert_for_run(run.id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: format!("delivery alert for run {} was not persisted", run.id),
            })
    }

    pub async fn resolve_delivery_alert(&self, run_id: i64) -> Result<(), StorageError> {
        let now = unix_millis_now();
        sqlx::query(
            r#"
            UPDATE delivery_alerts
            SET status = 'resolved', resolved_at = ?, updated_at = ?
            WHERE run_id = ? AND status != 'resolved'
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(run_id)
        .execute(&self.accounts)
        .await?;
        Ok(())
    }

    /// Mark system/in-app notification as delivered once. Returns true if this call won the race.
    pub async fn mark_delivery_alert_system_notified(
        &self,
        alert_id: i64,
    ) -> Result<bool, StorageError> {
        let now = unix_millis_now();
        let result = sqlx::query(
            r#"
            UPDATE delivery_alerts
            SET system_notified_at = ?, updated_at = ?
            WHERE id = ? AND status = 'open' AND system_notified_at = 0
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(alert_id)
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Mark external channel notification (Feishu/etc.) as delivered once.
    pub async fn mark_delivery_alert_external_notified(
        &self,
        alert_id: i64,
    ) -> Result<bool, StorageError> {
        let now = unix_millis_now();
        let result = sqlx::query(
            r#"
            UPDATE delivery_alerts
            SET external_notified_at = ?, updated_at = ?
            WHERE id = ? AND status = 'open' AND external_notified_at = 0
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(alert_id)
        .execute(&self.accounts)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    async fn delivery_alert_for_run(
        &self,
        run_id: i64,
    ) -> Result<Option<DeliveryAlert>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT
                id, run_id, uid, status, title, body, system_notified_at,
                external_notified_at, resolved_at, created_at, updated_at
            FROM delivery_alerts
            WHERE run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_optional(&self.accounts)
        .await?;
        Ok(row.map(delivery_alert_from_row))
    }
}

#[derive(Debug, Clone)]
struct DeliveryRunSnapshotFields {
    pool_owner_key: String,
    pool_scope_snapshot: String,
    one_time_pool_tags: Vec<String>,
    fixed_pool_tags: Vec<String>,
    fixed_message_snapshot: String,
}

fn normalize_pool_scope(scope: &str) -> Result<&'static str, StorageError> {
    match scope.trim().to_ascii_lowercase().as_str() {
        "account" => Ok("account"),
        "global" => Ok("global"),
        _ => Err(StorageError::Conflict {
            message: "卡密池范围无效".to_string(),
        }),
    }
}

fn serialize_string_vec(values: &[String]) -> Result<String, StorageError> {
    serde_json::to_string(values).map_err(|error| {
        StorageError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to serialize delivery run snapshot tags: {error}"),
        ))
    })
}

fn parse_run_tags_json(raw: &str, field: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str::<Vec<String>>(raw).map_err(|error| {
        StorageError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("delivery run {field} is corrupt: {error}"),
        ))
    })
}

async fn delivery_run_snapshot_from_rule_tx(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    storage: &StorageState,
    input: &NewDeliveryRun,
    allow_missing_rule_default: bool,
) -> Result<DeliveryRunSnapshotFields, StorageError> {
    let rule_row = sqlx::query(
        r#"
        SELECT
            cookie_id,
            card_pool_scope,
            card_pool_tag,
            fixed_message,
            COALESCE(one_time_enabled, 0) AS one_time_enabled,
            COALESCE(
                (SELECT GROUP_CONCAT(pool_tag, char(31))
                 FROM delivery_rule_card_pools p
                 WHERE p.rule_id = delivery_rules.id
                 ORDER BY position),
                ''
            ) AS pool_tags,
            COALESCE(
                (SELECT GROUP_CONCAT(pool_tag, char(31))
                 FROM delivery_rule_fixed_card_pools p
                 WHERE p.rule_id = delivery_rules.id
                 ORDER BY position),
                ''
            ) AS fixed_pool_tags
        FROM delivery_rules
        WHERE id = ?
        "#,
    )
    .bind(input.rule_id)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(rule_row) = rule_row else {
        if !allow_missing_rule_default {
            return Err(StorageError::Conflict {
                message: format!(
                    "delivery rule {} is missing while backfilling run snapshot",
                    input.rule_id
                ),
            });
        }
        return Ok(DeliveryRunSnapshotFields {
            pool_owner_key: input.uid.trim().to_string(),
            pool_scope_snapshot: "account".to_string(),
            one_time_pool_tags: Vec::new(),
            fixed_pool_tags: Vec::new(),
            fixed_message_snapshot: String::new(),
        });
    };

    let rule_uid = rule_row.get::<String, _>("cookie_id");
    let pool_scope_snapshot =
        normalize_pool_scope(&rule_row.get::<String, _>("card_pool_scope"))?.to_string();
    let one_time_enabled = rule_row.get::<i64, _>("one_time_enabled") != 0;
    let legacy_pool_tag = rule_row.get::<String, _>("card_pool_tag");
    let one_time_pool_tags = if one_time_enabled {
        normalize_pool_tags(
            &rule_row
                .get::<String, _>("pool_tags")
                .split('\u{1f}')
                .filter(|tag| !tag.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>(),
            &legacy_pool_tag,
        )
    } else {
        Vec::new()
    };
    let fixed_pool_tags = normalize_pool_tags(
        &rule_row
            .get::<String, _>("fixed_pool_tags")
            .split('\u{1f}')
            .filter(|tag| !tag.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        "",
    );
    let pool_owner_key = storage
        .resolve_card_pool_cookie_id(&rule_uid, &pool_scope_snapshot)
        .await?;

    Ok(DeliveryRunSnapshotFields {
        pool_owner_key,
        pool_scope_snapshot,
        one_time_pool_tags,
        fixed_pool_tags,
        fixed_message_snapshot: rule_row.get::<String, _>("fixed_message"),
    })
}

fn run_core_snapshot_missing(run: &DeliveryRun) -> bool {
    run.pool_owner_key.trim().is_empty() || run.pool_scope_snapshot.trim().is_empty()
}

fn run_fixed_message_snapshot_missing(run: &DeliveryRun) -> bool {
    !run.fixed_message_snapshot_captured
}

fn run_snapshot_missing(run: &DeliveryRun) -> bool {
    run_core_snapshot_missing(run) || run_fixed_message_snapshot_missing(run)
}

fn validate_explicit_reservation_against_snapshot(
    run: &DeliveryRun,
    uid: &str,
    pool_scope: &str,
    card_pool_tags: &[String],
    required: i64,
) -> Result<(), StorageError> {
    let requested_scope = normalize_pool_scope(pool_scope)?;
    let requested_tags = normalize_pool_tags(card_pool_tags, "");
    let tags_match = if run.one_time_pool_tags.is_empty() {
        requested_tags.is_empty() && required == 0
    } else {
        run.one_time_pool_tags == requested_tags && run.required_card_count == required
    };
    if run.uid.trim() != uid.trim() || run.pool_scope_snapshot != requested_scope || !tags_match {
        return Err(StorageError::Conflict {
            message: format!(
                "delivery run {} explicit reservation arguments do not match its frozen snapshot",
                run.id
            ),
        });
    }
    Ok(())
}

fn can_backfill_snapshot(run: &DeliveryRun) -> bool {
    matches!(
        run.status,
        DeliveryRunStatus::Pending | DeliveryRunStatus::WaitingStock | DeliveryRunStatus::Retrying
    ) && run.waiting_notice_sent_at == 0
        && run.waiting_notice_claimed_at == 0
        && run.admin_notified_at == 0
        && run.consigned_at == 0
        && run.message_sent_at == 0
        && run.delivery_send_claimed_at == 0
        && run.completed_at == 0
        && run.cancel_observed_at == 0
}

fn quantity_retry_at(attempt_count: i64) -> i64 {
    let exponent = attempt_count.clamp(0, 5) as u32;
    unix_millis_now() + 30_000_i64.saturating_mul(1_i64 << exponent)
}

async fn load_delivery_run_row(
    executor: impl sqlx::Executor<'_, Database = Sqlite>,
    run_id: i64,
) -> Result<Option<SqliteRow>, sqlx::Error> {
    sqlx::query(
        r#"
        SELECT
            id, uid, rule_id, order_id, buyer_id, item_id, pool_owner_key,
            pool_scope_snapshot, one_time_pool_tags_json, fixed_pool_tags_json,
            fixed_message_snapshot, fixed_message_snapshot_captured, purchase_quantity,
            purchase_quantity_source, required_card_count, quantity_confirmed_at, status, attempt_count, waiting_notice_sent_at,
            waiting_notice_claimed_at, waiting_notice_claim_owner, waiting_notice_claim_expires_at,
            admin_notified_at, consigned_at, message_sent_at, delivery_send_claimed_at,
            delivery_send_claim_owner, delivery_send_claim_expires_at, delivery_send_started_at,
            lease_owner, lease_expires_at, message_acknowledged_at, message_ack_id,
            message_ack_uuid, completed_at, cancel_observed_at, next_retry_at,
            last_error_code, last_error_message, created_at, updated_at
        FROM delivery_runs
        WHERE id = ?
        "#,
    )
    .bind(run_id)
    .fetch_optional(executor)
    .await
}

async fn maybe_backfill_run_snapshot(
    storage: &StorageState,
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    run: &DeliveryRun,
) -> Result<(), StorageError> {
    if !run_snapshot_missing(run) {
        return Ok(());
    }

    let reserved_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delivery_run_cards WHERE run_id = ?")
            .bind(run.id)
            .fetch_one(&mut **tx)
            .await?;
    if can_backfill_snapshot(run) && reserved_count == 0 {
        if run_core_snapshot_missing(run) {
            let snapshot = delivery_run_snapshot_from_rule_tx(
                tx,
                storage,
                &NewDeliveryRun {
                    uid: run.uid.clone(),
                    rule_id: run.rule_id,
                    order_id: run.order_id.clone(),
                    buyer_id: run.buyer_id.clone(),
                    item_id: run.item_id.clone(),
                    purchase_quantity: run.purchase_quantity,
                    purchase_quantity_source: run.purchase_quantity_source.clone(),
                    required_card_count: run.required_card_count,
                },
                false,
            )
            .await;
            let Ok(snapshot) = snapshot else {
                mark_run_snapshot_missing(tx, run.id).await?;
                return Ok(());
            };
            let result = sqlx::query(
                r#"
                UPDATE delivery_runs
                SET pool_owner_key = ?, pool_scope_snapshot = ?,
                    one_time_pool_tags_json = ?, fixed_pool_tags_json = ?,
                    fixed_message_snapshot = ?, fixed_message_snapshot_captured = 1,
                    updated_at = ?
                WHERE id = ?
                  AND TRIM(pool_owner_key) = ''
                  AND TRIM(pool_scope_snapshot) = ''
                "#,
            )
            .bind(&snapshot.pool_owner_key)
            .bind(&snapshot.pool_scope_snapshot)
            .bind(serialize_string_vec(&snapshot.one_time_pool_tags)?)
            .bind(serialize_string_vec(&snapshot.fixed_pool_tags)?)
            .bind(snapshot.fixed_message_snapshot.trim())
            .bind(unix_millis_now())
            .bind(run.id)
            .execute(&mut **tx)
            .await?;
            let Some(reloaded) = load_delivery_run_row(&mut **tx, run.id).await? else {
                return Ok(());
            };
            let reloaded = delivery_run_from_row(reloaded)?;
            if !run_snapshot_missing(&reloaded) {
                return Ok(());
            }
            if result.rows_affected() == 0 {
                mark_run_snapshot_missing(tx, run.id).await?;
                return Ok(());
            }
            mark_run_snapshot_missing(tx, run.id).await?;
            return Ok(());
        }

        if run_fixed_message_snapshot_missing(run) {
            let Some(fixed_message_snapshot) =
                load_rule_fixed_message_snapshot_tx(tx, run.rule_id).await?
            else {
                mark_run_snapshot_missing(tx, run.id).await?;
                return Ok(());
            };
            let result = sqlx::query(
                r#"
                UPDATE delivery_runs
                SET fixed_message_snapshot = CASE
                        WHEN TRIM(fixed_message_snapshot) = '' THEN ?
                        ELSE fixed_message_snapshot
                    END,
                    fixed_message_snapshot_captured = 1,
                    updated_at = ?
                WHERE id = ? AND fixed_message_snapshot_captured = 0
                "#,
            )
            .bind(fixed_message_snapshot.trim())
            .bind(unix_millis_now())
            .bind(run.id)
            .execute(&mut **tx)
            .await?;
            let Some(reloaded) = load_delivery_run_row(&mut **tx, run.id).await? else {
                return Ok(());
            };
            let reloaded = delivery_run_from_row(reloaded)?;
            if !run_fixed_message_snapshot_missing(&reloaded) {
                return Ok(());
            }
            if result.rows_affected() == 0 {
                mark_run_snapshot_missing(tx, run.id).await?;
                return Ok(());
            }
            mark_run_snapshot_missing(tx, run.id).await?;
            return Ok(());
        }
    }

    if run_snapshot_missing(run) {
        mark_run_snapshot_missing(tx, run.id).await?;
    }
    Ok(())
}

async fn load_rule_fixed_message_snapshot_tx(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    rule_id: i64,
) -> Result<Option<String>, StorageError> {
    sqlx::query_scalar::<_, String>("SELECT fixed_message FROM delivery_rules WHERE id = ?")
        .bind(rule_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(StorageError::from)
}

async fn mark_run_snapshot_missing(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    run_id: i64,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        UPDATE delivery_runs
        SET status = 'manual_required',
            last_error_code = 'snapshot_missing',
            last_error_message = '执行记录缺少稳定配置快照，请人工处理',
            next_retry_at = 0,
            updated_at = ?
        WHERE id = ? AND status != 'manual_required'
        "#,
    )
    .bind(unix_millis_now())
    .bind(run_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn mark_run_reserved(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: i64,
) -> Result<(), StorageError> {
    let updated = sqlx::query(
        r#"
        UPDATE delivery_runs
        SET status = 'reserved', next_retry_at = 0,
            last_error_code = '', last_error_message = '', updated_at = ?
        WHERE id = ? AND status IN ('pending', 'waiting_stock', 'retrying')
        "#,
    )
    .bind(unix_millis_now())
    .bind(run_id)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        let status =
            sqlx::query_scalar::<_, String>("SELECT status FROM delivery_runs WHERE id = ?")
                .bind(run_id)
                .fetch_optional(&mut **tx)
                .await?;
        if status.as_deref() == Some("reserved") {
            return Ok(());
        }
        return Err(StorageError::Conflict {
            message: format!("delivery run {run_id} cannot transition to reserved"),
        });
    }
    Ok(())
}

async fn insert_delivery_cards(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    uid: &str,
    pool_tag: &str,
    cards: &[String],
    now: i64,
) -> Result<i64, StorageError> {
    let mut inserted = 0_i64;
    for content in cards {
        let result = sqlx::query(
            r#"
            INSERT OR IGNORE INTO cards (
                cookie_id, card_key, card_content, card_pool_tag, reusable,
                used, used_at, used_order_id, created_at
            ) VALUES (?, '', ?, ?, 0, 0, 0, '', ?)
            "#,
        )
        .bind(uid)
        .bind(content)
        .bind(pool_tag)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        inserted += result.rows_affected() as i64;
    }
    Ok(inserted)
}

async fn enable_auto_ship(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    uid: &str,
    now: i64,
) -> Result<(), StorageError> {
    set_auto_ship_enabled(tx, uid, true, now).await
}

async fn set_auto_ship_enabled(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    uid: &str,
    enabled: bool,
    now: i64,
) -> Result<(), StorageError> {
    let uid = uid.trim();
    if uid.is_empty() {
        return Ok(());
    }
    let flag = if enabled { 1_i64 } else { 0_i64 };
    sqlx::query(
        r#"
        INSERT INTO ai_reply_settings (cookie_id, auto_ship_enabled, updated_at)
        VALUES (?, ?, ?)
        ON CONFLICT(cookie_id) DO UPDATE SET
            auto_ship_enabled = excluded.auto_ship_enabled,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(uid)
    .bind(flag)
    .bind(now)
    .execute(&mut **tx)
    .await?;

    // Keep accounts.extra.autoShip aligned with the runtime flag so account list toggles
    // do not show "on" when no delivery configuration remains.
    let extra_raw =
        sqlx::query_scalar::<_, String>("SELECT extra FROM accounts WHERE uid = ? LIMIT 1")
            .bind(uid)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(raw) = extra_raw {
        let mut extra = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&raw)
            .unwrap_or_default();
        extra.insert("autoShip".to_string(), serde_json::Value::Bool(enabled));
        let serialized = serde_json::to_string(&extra).map_err(|error| StorageError::Conflict {
            message: format!("写入账号自动发货开关失败: {error}"),
        })?;
        sqlx::query("UPDATE accounts SET extra = ?, updated_at = ? WHERE uid = ?")
            .bind(serialized)
            .bind((now as f64) / 1000.0)
            .bind(uid)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

fn delivery_configuration_from_row(row: SqliteRow) -> DeliveryConfigurationRecord {
    let legacy_pool_tag = row.get::<String, _>("card_pool_tag");
    let one_time_enabled = row.get::<i64, _>("one_time_enabled") != 0;
    let mut pool_tags = if one_time_enabled {
        row.get::<String, _>("pool_tags")
            .split('\u{1f}')
            .filter(|tag| !tag.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let fixed_pool_tags = row
        .get::<String, _>("fixed_pool_tags")
        .split('\u{1f}')
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect();
    if one_time_enabled && pool_tags.is_empty() && !legacy_pool_tag.trim().is_empty() {
        pool_tags.push(legacy_pool_tag.clone());
    }
    DeliveryConfigurationRecord {
        id: row.get("id"),
        uid: row.get("cookie_id"),
        item_id: row.get("item_id"),
        fixed_message: row.get("fixed_message"),
        one_time_enabled,
        enabled: row.get::<i64, _>("enabled") != 0,
        needs_repair: row.get::<i64, _>("needs_repair") != 0,
        pool_scope: row.get("card_pool_scope"),
        pool_tag: legacy_pool_tag,
        pool_tags,
        fixed_pool_tags,
        created_at: row.get::<f64, _>("created_at") as i64,
    }
}

async fn replace_delivery_rule_fixed_pools(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    rule_id: i64,
    pool_tags: &[String],
    now: i64,
) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM delivery_rule_fixed_card_pools WHERE rule_id = ?")
        .bind(rule_id)
        .execute(&mut **tx)
        .await?;
    for (position, pool_tag) in pool_tags.iter().enumerate() {
        sqlx::query("INSERT INTO delivery_rule_fixed_card_pools (rule_id, pool_tag, position, created_at) VALUES (?, ?, ?, ?)").bind(rule_id).bind(pool_tag).bind(position as i64).bind(now).execute(&mut **tx).await?;
    }
    Ok(())
}

fn normalize_pool_tags(pool_tags: &[String], legacy_pool_tag: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for tag in pool_tags
        .iter()
        .map(|tag| tag.trim())
        .chain(std::iter::once(legacy_pool_tag.trim()))
    {
        if !tag.is_empty() && !tags.iter().any(|existing| existing == tag) {
            tags.push(tag.to_string());
        }
    }
    tags
}

async fn replace_delivery_rule_pools(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    rule_id: i64,
    pool_tags: &[String],
    now: i64,
) -> Result<(), StorageError> {
    sqlx::query("DELETE FROM delivery_rule_card_pools WHERE rule_id = ?")
        .bind(rule_id)
        .execute(&mut **tx)
        .await?;
    for (position, pool_tag) in pool_tags.iter().enumerate() {
        sqlx::query("INSERT INTO delivery_rule_card_pools (rule_id, pool_tag, position, created_at) VALUES (?, ?, ?, ?)")
            .bind(rule_id).bind(pool_tag).bind(position as i64).bind(now).execute(&mut **tx).await?;
    }
    Ok(())
}

fn delivery_run_summary_from_row(row: SqliteRow) -> DeliveryRunSummary {
    let last_error_code = row.get::<String, _>("last_error_code");
    DeliveryRunSummary {
        id: row.get("id"),
        uid: row.get("uid"),
        configuration_id: row.get("rule_id"),
        order_id: row.get("order_id"),
        buyer_id: row.get("buyer_id"),
        item_id: row.get("item_id"),
        purchase_quantity: row.get("purchase_quantity"),
        required_card_count: row.get("required_card_count"),
        status: row.get("status"),
        attempt_count: row.get("attempt_count"),
        last_error_message: public_delivery_run_error(&last_error_code),
        last_error_code,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn is_ready_to_auto_delivery(status: &str, status_text: &str) -> bool {
    let status = status.trim().to_ascii_uppercase();
    let status_text = status_text.trim();
    if status.contains("CANCEL")
        || status.contains("CLOSE")
        || status.contains("REFUND")
        || status.contains("SUCCESS")
        || status.contains("CONSIGNED")
        || status_text.contains("已发货")
        || status_text.contains("已完成")
        || status_text.contains("已关闭")
        || status_text.contains("退款")
    {
        return false;
    }
    status.contains("WAIT_SELLER_SEND")
        || status.contains("WAIT_SELLER_CONSIGN")
        || status_text.contains("待发货")
        || status_text.contains("等待发货")
        || status_text.contains("等待卖家发货")
        || status_text.contains("去发货")
}

fn is_closed_or_refunded_order(status: &str, status_text: &str) -> bool {
    let status = status.trim().to_ascii_uppercase();
    let status_text = status_text.trim();
    status.contains("CANCEL")
        || status.contains("CLOSE")
        || status.contains("REFUND")
        || status_text.contains("已关闭")
        || status_text.contains("交易关闭")
        || status_text.contains("退款")
}

fn public_delivery_run_error(code: &str) -> String {
    match code.trim() {
        "" => String::new(),
        "card_stock_insufficient" | "insufficient_stock" => "一次性卡密库存不足".to_string(),
        "quantity_unknown" | "quantity_unknown_retryable" => "订单购买数量暂时无法确认".to_string(),
        "snapshot_missing" => "执行记录缺少稳定配置快照".to_string(),
        "item_unavailable" => "自动发货商品已下架或售罄".to_string(),
        "upstream_temporary" | "network_temporary" => {
            "平台服务暂时不可用，系统将稍后重试".to_string()
        }
        _ => "自动发货执行失败，请查看日志或稍后重试".to_string(),
    }
}

fn requires_manual_delivery_confirmation(code: &str) -> bool {
    // Only block retry when message/consign side-effects may have already partially happened
    // and replaying would double-send. Pure "already shipped by user" cases can continue.
    matches!(
        code.trim(),
        "send_result_uncertain"
            | "message_ack_uncertain"
            | "waiting_notice_result_uncertain"
            | "waiting_notice_ack_uncertain"
            | "consign_result_uncertain"
            | "quantity_unknown"
            | "snapshot_missing"
            | "configuration_missing"
            | "delivery_message_empty"
    )
}

async fn load_delivery_run_cards(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: i64,
) -> Result<Vec<DeliveryRunCard>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT run_id, card_id, position, content_snapshot, status, created_at, updated_at
        FROM delivery_run_cards
        WHERE run_id = ?
        ORDER BY position ASC
        "#,
    )
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(delivery_run_card_from_row).collect())
}

fn delivery_run_from_row(row: SqliteRow) -> Result<DeliveryRun, StorageError> {
    let raw_status = row.get::<String, _>("status");
    let status = DeliveryRunStatus::from_str(&raw_status).map_err(|message| {
        StorageError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        ))
    })?;
    let one_time_pool_tags = parse_run_tags_json(
        &row.get::<String, _>("one_time_pool_tags_json"),
        "one_time_pool_tags_json",
    )?;
    let fixed_pool_tags = parse_run_tags_json(
        &row.get::<String, _>("fixed_pool_tags_json"),
        "fixed_pool_tags_json",
    )?;
    Ok(DeliveryRun {
        id: row.get("id"),
        uid: row.get("uid"),
        rule_id: row.get("rule_id"),
        order_id: row.get("order_id"),
        buyer_id: row.get("buyer_id"),
        item_id: row.get("item_id"),
        pool_owner_key: row.get("pool_owner_key"),
        pool_scope_snapshot: row.get("pool_scope_snapshot"),
        one_time_pool_tags,
        fixed_pool_tags,
        fixed_message_snapshot: row.get("fixed_message_snapshot"),
        fixed_message_snapshot_captured: row.get::<i64, _>("fixed_message_snapshot_captured") != 0,
        purchase_quantity: row.get("purchase_quantity"),
        purchase_quantity_source: row.get("purchase_quantity_source"),
        required_card_count: row.get("required_card_count"),
        quantity_confirmed_at: row.get("quantity_confirmed_at"),
        status,
        attempt_count: row.get("attempt_count"),
        waiting_notice_sent_at: row.get("waiting_notice_sent_at"),
        waiting_notice_claimed_at: row.get("waiting_notice_claimed_at"),
        waiting_notice_claim_owner: row.get("waiting_notice_claim_owner"),
        waiting_notice_claim_expires_at: row.get("waiting_notice_claim_expires_at"),
        admin_notified_at: row.get("admin_notified_at"),
        consigned_at: row.get("consigned_at"),
        message_sent_at: row.get("message_sent_at"),
        delivery_send_claimed_at: row.get("delivery_send_claimed_at"),
        delivery_send_claim_owner: row.get("delivery_send_claim_owner"),
        delivery_send_claim_expires_at: row.get("delivery_send_claim_expires_at"),
        delivery_send_started_at: row.get("delivery_send_started_at"),
        lease_owner: row.get("lease_owner"),
        lease_expires_at: row.get("lease_expires_at"),
        message_acknowledged_at: row.get("message_acknowledged_at"),
        message_ack_id: row.get("message_ack_id"),
        message_ack_uuid: row.get("message_ack_uuid"),
        completed_at: row.get("completed_at"),
        cancel_observed_at: row.get("cancel_observed_at"),
        next_retry_at: row.get("next_retry_at"),
        last_error_code: row.get("last_error_code"),
        last_error_message: row.get("last_error_message"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn delivery_run_card_from_row(row: SqliteRow) -> DeliveryRunCard {
    DeliveryRunCard {
        run_id: row.get("run_id"),
        card_id: row.get("card_id"),
        position: row.get("position"),
        content_snapshot: row.get("content_snapshot"),
        status: row.get("status"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn delivery_alert_from_row(row: SqliteRow) -> DeliveryAlert {
    DeliveryAlert {
        id: row.get("id"),
        run_id: row.get("run_id"),
        uid: row.get("uid"),
        status: row.get("status"),
        title: row.get("title"),
        body: row.get("body"),
        system_notified_at: row.get("system_notified_at"),
        external_notified_at: row.get("external_notified_at"),
        resolved_at: row.get("resolved_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn delivery_discovery_alert_from_row(row: SqliteRow) -> DeliveryDiscoveryAlert {
    DeliveryDiscoveryAlert {
        id: row.get("id"),
        alert_key: row.get("alert_key"),
        alert_type: row.get("alert_type"),
        uid: row.get("uid"),
        rule_id: row.get("rule_id"),
        order_id: row.get("order_id"),
        item_id: row.get("item_id"),
        order_status: row.get("order_status"),
        order_status_text: row.get("order_status_text"),
        status: row.get("status"),
        title: row.get("title"),
        body: row.get("body"),
        occurrence_count: row.get("occurrence_count"),
        first_seen_at: row.get("first_seen_at"),
        last_seen_at: row.get("last_seen_at"),
        resolved_at: row.get("resolved_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::is_closed_or_refunded_order;

    #[test]
    fn identifies_closed_and_refunded_orders_for_missed_run_reconciliation() {
        assert!(is_closed_or_refunded_order("close_by_buyer", "交易关闭"));
        assert!(is_closed_or_refunded_order(
            "refund_success",
            "交易关闭，有退款"
        ));
        assert!(!is_closed_or_refunded_order(
            "seller_to_send",
            "等待卖家发货"
        ));
        assert!(!is_closed_or_refunded_order("trade_success", "交易成功"));
    }
}
