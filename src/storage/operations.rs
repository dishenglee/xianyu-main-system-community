use serde_json::{json, Value};
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::models::operations::{
    MessageDailyStat, MessageDailyStatFilter, OperationJob, OperationJobItem, OperationLogEntry,
    OperationLogFilter,
};

use super::{unix_millis_now, StorageError, StorageState};

#[derive(Debug, Clone)]
pub struct NewOperationLog<'a> {
    pub job_id: &'a str,
    pub module: &'a str,
    pub action: &'a str,
    pub operator: &'a str,
    pub trigger_source: &'a str,
    pub uid: &'a str,
    pub account_nick: &'a str,
    pub target_type: &'a str,
    pub target_id: &'a str,
    pub target_title: &'a str,
    pub status: &'a str,
    pub error_code: &'a str,
    pub message: &'a str,
    pub params: Value,
    pub before: Value,
    pub after: Value,
    pub extra: Value,
}

#[derive(Debug, Clone)]
pub struct NewOperationJob<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub module: &'a str,
    pub action: &'a str,
    pub operator: &'a str,
    pub trigger_source: &'a str,
    pub status: &'a str,
    pub params: Value,
    pub summary: Value,
    pub total_count: i64,
}

#[derive(Debug, Clone)]
pub struct NewOperationJobItem<'a> {
    pub job_id: &'a str,
    pub seq: i64,
    pub uid: &'a str,
    pub account_nick: &'a str,
    pub target_type: &'a str,
    pub target_id: &'a str,
    pub target_title: &'a str,
    pub status: &'a str,
    pub params: Value,
}

fn parse_json(text: String) -> Value {
    serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
}

fn log_from_row(row: sqlx::sqlite::SqliteRow) -> OperationLogEntry {
    OperationLogEntry {
        id: row.get("id"),
        job_id: row.get("job_id"),
        module: row.get("module"),
        action: row.get("action"),
        operator: row.get("operator"),
        trigger_source: row.get("trigger_source"),
        uid: row.get("uid"),
        account_nick: row.get("account_nick"),
        target_type: row.get("target_type"),
        target_id: row.get("target_id"),
        target_title: row.get("target_title"),
        status: row.get("status"),
        error_code: row.get("error_code"),
        message: row.get("message"),
        params: parse_json(row.get("params_json")),
        before: parse_json(row.get("before_json")),
        after: parse_json(row.get("after_json")),
        extra: parse_json(row.get("extra_json")),
        created_at: row.get("created_at"),
    }
}

fn job_from_row(row: sqlx::sqlite::SqliteRow) -> OperationJob {
    OperationJob {
        id: row.get("id"),
        kind: row.get("kind"),
        module: row.get("module"),
        action: row.get("action"),
        operator: row.get("operator"),
        trigger_source: row.get("trigger_source"),
        status: row.get("status"),
        params: parse_json(row.get("params_json")),
        summary: parse_json(row.get("summary_json")),
        total_count: row.get("total_count"),
        success_count: row.get("success_count"),
        fail_count: row.get("fail_count"),
        skipped_count: row.get("skipped_count"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        last_error: row.get("last_error"),
    }
}

fn job_item_from_row(row: sqlx::sqlite::SqliteRow) -> OperationJobItem {
    OperationJobItem {
        id: row.get("id"),
        job_id: row.get("job_id"),
        seq: row.get("seq"),
        uid: row.get("uid"),
        account_nick: row.get("account_nick"),
        target_type: row.get("target_type"),
        target_id: row.get("target_id"),
        target_title: row.get("target_title"),
        status: row.get("status"),
        params: parse_json(row.get("params_json")),
        result: parse_json(row.get("result_json")),
        message: row.get("message"),
        error_code: row.get("error_code"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn message_stat_from_row(row: sqlx::sqlite::SqliteRow) -> MessageDailyStat {
    MessageDailyStat {
        stat_date: row.get("stat_date"),
        uid: row.get("uid"),
        account_nick: row.get("account_nick"),
        notification_message_count: row.get("notification_message_count"),
        interaction_message_count: row.get("interaction_message_count"),
        received_message_count: row.get("received_message_count"),
        default_reply_count: row.get("default_reply_count"),
        auto_reply_count: row.get("auto_reply_count"),
        ai_reply_count: row.get("ai_reply_count"),
        manual_reply_count: row.get("manual_reply_count"),
        image_reply_count: row.get("image_reply_count"),
        auto_delivery_count: row.get("auto_delivery_count"),
        reply_failure_count: row.get("reply_failure_count"),
        recall_count: row.get("recall_count"),
        updated_at: row.get("updated_at"),
    }
}

impl StorageState {
    pub async fn append_operation_log(
        &self,
        entry: NewOperationLog<'_>,
    ) -> Result<i64, StorageError> {
        let result = sqlx::query(
            r#"
            INSERT INTO operation_logs (
                job_id, module, action, operator, trigger_source, uid, account_nick,
                target_type, target_id, target_title, status, error_code, message,
                params_json, before_json, after_json, extra_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'))
            "#,
        )
        .bind(entry.job_id)
        .bind(entry.module)
        .bind(entry.action)
        .bind(entry.operator)
        .bind(entry.trigger_source)
        .bind(entry.uid)
        .bind(entry.account_nick)
        .bind(entry.target_type)
        .bind(entry.target_id)
        .bind(entry.target_title)
        .bind(entry.status)
        .bind(entry.error_code)
        .bind(entry.message)
        .bind(entry.params.to_string())
        .bind(entry.before.to_string())
        .bind(entry.after.to_string())
        .bind(entry.extra.to_string())
        .execute(&self.xianyu)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn list_operation_logs(
        &self,
        filter: &OperationLogFilter,
    ) -> Result<Vec<OperationLogEntry>, StorageError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id, job_id, module, action, operator, trigger_source, uid, account_nick, \
             target_type, target_id, target_title, status, error_code, message, params_json, \
             before_json, after_json, extra_json, created_at FROM operation_logs WHERE 1=1",
        );
        if !filter.module.trim().is_empty() {
            builder
                .push(" AND module = ")
                .push_bind(filter.module.trim());
        }
        if !filter.action.trim().is_empty() {
            builder
                .push(" AND action = ")
                .push_bind(filter.action.trim());
        }
        if !filter.uid.trim().is_empty() {
            builder.push(" AND uid = ").push_bind(filter.uid.trim());
        }
        if !filter.status.trim().is_empty() {
            builder
                .push(" AND status = ")
                .push_bind(filter.status.trim());
        }
        if !filter.keyword.trim().is_empty() {
            let keyword = format!("%{}%", filter.keyword.trim());
            builder
                .push(" AND (message LIKE ")
                .push_bind(keyword.clone());
            builder
                .push(" OR target_title LIKE ")
                .push_bind(keyword.clone());
            builder
                .push(" OR account_nick LIKE ")
                .push_bind(keyword.clone());
            builder.push(" OR target_id LIKE ").push_bind(keyword);
            builder.push(")");
        }
        if filter.since > 0.0 {
            builder.push(" AND created_at >= ").push_bind(filter.since);
        }
        builder
            .push(" ORDER BY created_at DESC LIMIT ")
            .push_bind(filter.limit.max(1).min(500));
        let rows = builder.build().fetch_all(&self.xianyu).await?;
        Ok(rows.into_iter().map(log_from_row).collect())
    }

    pub async fn create_operation_job(&self, job: NewOperationJob<'_>) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO operation_jobs (
                id, kind, module, action, operator, trigger_source, status,
                params_json, summary_json, total_count, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'), unixepoch('subsec'))
            "#,
        )
        .bind(job.id)
        .bind(job.kind)
        .bind(job.module)
        .bind(job.action)
        .bind(job.operator)
        .bind(job.trigger_source)
        .bind(job.status)
        .bind(job.params.to_string())
        .bind(job.summary.to_string())
        .bind(job.total_count)
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    pub async fn add_operation_job_items(
        &self,
        items: &[NewOperationJobItem<'_>],
    ) -> Result<(), StorageError> {
        for item in items {
            sqlx::query(
                r#"
                INSERT INTO operation_job_items (
                    job_id, seq, uid, account_nick, target_type, target_id, target_title,
                    status, params_json, result_json, message, error_code, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', '', '', unixepoch('subsec'), unixepoch('subsec'))
                "#,
            )
            .bind(item.job_id)
            .bind(item.seq)
            .bind(item.uid)
            .bind(item.account_nick)
            .bind(item.target_type)
            .bind(item.target_id)
            .bind(item.target_title)
            .bind(item.status)
            .bind(item.params.to_string())
            .execute(&self.xianyu)
            .await?;
        }
        Ok(())
    }

    pub async fn get_operation_job(&self, id: &str) -> Result<Option<OperationJob>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, kind, module, action, operator, trigger_source, status,
                   params_json, summary_json, total_count, success_count, fail_count,
                   skipped_count, started_at, finished_at, created_at, updated_at, last_error
            FROM operation_jobs
            WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.xianyu)
        .await?;
        Ok(row.map(job_from_row))
    }

    pub async fn list_operation_jobs(&self, limit: i64) -> Result<Vec<OperationJob>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, kind, module, action, operator, trigger_source, status,
                   params_json, summary_json, total_count, success_count, fail_count,
                   skipped_count, started_at, finished_at, created_at, updated_at, last_error
            FROM operation_jobs
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit.max(1).min(200))
        .fetch_all(&self.xianyu)
        .await?;
        Ok(rows.into_iter().map(job_from_row).collect())
    }

    pub async fn claim_next_operation_job(&self) -> Result<Option<OperationJob>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id
            FROM operation_jobs
            WHERE status = 'queued'
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.xianyu)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let id: String = row.get("id");
        let now = unix_millis_now() as f64 / 1000.0;
        let updated = sqlx::query(
            "UPDATE operation_jobs SET status = 'running', started_at = ?, updated_at = ? WHERE id = ? AND status = 'queued'",
        )
        .bind(now)
        .bind(now)
        .bind(&id)
        .execute(&self.xianyu)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_operation_job(&id).await
    }

    pub async fn list_operation_job_items(
        &self,
        job_id: &str,
    ) -> Result<Vec<OperationJobItem>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, job_id, seq, uid, account_nick, target_type, target_id, target_title,
                   status, params_json, result_json, message, error_code,
                   started_at, finished_at, created_at, updated_at
            FROM operation_job_items
            WHERE job_id = ?
            ORDER BY seq ASC, id ASC
            "#,
        )
        .bind(job_id)
        .fetch_all(&self.xianyu)
        .await?;
        Ok(rows.into_iter().map(job_item_from_row).collect())
    }

    pub async fn update_operation_job_item(
        &self,
        id: i64,
        status: &str,
        result: Value,
        message: &str,
        error_code: &str,
        started_at: Option<f64>,
        finished_at: Option<f64>,
    ) -> Result<(), StorageError> {
        let now = unix_millis_now() as f64 / 1000.0;
        sqlx::query(
            r#"
            UPDATE operation_job_items
            SET status = ?, result_json = ?, message = ?, error_code = ?,
                started_at = COALESCE(?, started_at), finished_at = COALESCE(?, finished_at), updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(result.to_string())
        .bind(message)
        .bind(error_code)
        .bind(started_at)
        .bind(finished_at)
        .bind(now)
        .bind(id)
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    pub async fn finalize_operation_job(
        &self,
        id: &str,
        status: &str,
        success_count: i64,
        fail_count: i64,
        skipped_count: i64,
        summary: Value,
        last_error: &str,
    ) -> Result<(), StorageError> {
        let now = unix_millis_now() as f64 / 1000.0;
        sqlx::query(
            r#"
            UPDATE operation_jobs
            SET status = ?, success_count = ?, fail_count = ?, skipped_count = ?,
                summary_json = ?, last_error = ?, finished_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(success_count)
        .bind(fail_count)
        .bind(skipped_count)
        .bind(summary.to_string())
        .bind(last_error)
        .bind(now)
        .bind(now)
        .bind(id)
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    pub async fn bump_message_daily_stat(
        &self,
        stat_date: &str,
        uid: &str,
        account_nick: &str,
        field: &str,
        delta: i64,
    ) -> Result<(), StorageError> {
        let allowed = [
            "notification_message_count",
            "interaction_message_count",
            "received_message_count",
            "default_reply_count",
            "auto_reply_count",
            "ai_reply_count",
            "manual_reply_count",
            "image_reply_count",
            "auto_delivery_count",
            "reply_failure_count",
            "recall_count",
        ];
        if !allowed.contains(&field) {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO message_daily_stats (stat_date, uid, account_nick, updated_at)
            VALUES (?, ?, ?, unixepoch('subsec'))
            ON CONFLICT(stat_date, uid) DO UPDATE SET
                account_nick = excluded.account_nick,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(stat_date)
        .bind(uid)
        .bind(account_nick)
        .execute(&self.xianyu)
        .await?;
        let sql = format!(
            "UPDATE message_daily_stats SET {field} = {field} + ?, updated_at = unixepoch('subsec') WHERE stat_date = ? AND uid = ?"
        );
        sqlx::query(&sql)
            .bind(delta)
            .bind(stat_date)
            .bind(uid)
            .execute(&self.xianyu)
            .await?;
        Ok(())
    }

    pub async fn list_message_daily_stats(
        &self,
        filter: &MessageDailyStatFilter,
    ) -> Result<Vec<MessageDailyStat>, StorageError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT stat_date, uid, account_nick, notification_message_count, interaction_message_count, \
             received_message_count, default_reply_count, auto_reply_count, ai_reply_count, manual_reply_count, \
             image_reply_count, auto_delivery_count, reply_failure_count, recall_count, updated_at \
             FROM message_daily_stats WHERE 1=1",
        );
        if !filter.uid.trim().is_empty() {
            builder.push(" AND uid = ").push_bind(filter.uid.trim());
        }
        if !filter.start_date.trim().is_empty() {
            builder
                .push(" AND stat_date >= ")
                .push_bind(filter.start_date.trim());
        }
        if !filter.end_date.trim().is_empty() {
            builder
                .push(" AND stat_date <= ")
                .push_bind(filter.end_date.trim());
        }
        builder
            .push(" ORDER BY stat_date DESC, uid ASC LIMIT ")
            .push_bind(filter.limit.max(1).min(365));
        let rows = builder.build().fetch_all(&self.xianyu).await?;
        Ok(rows.into_iter().map(message_stat_from_row).collect())
    }
}
