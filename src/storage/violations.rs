//! Account-violation persistence helpers.
//!
//! Owned table: `account_violations` (xianyu DB, migration 0008).
//!
//! Lifecycle:
//!   1. `items_sync` parses an upstream item card, runs `detect_violation`,
//!      and calls [`StorageState::record_violation`] when something matches.
//!   2. The UNIQUE (uid, item_id, violation_kind) constraint dedupes the
//!      same violation appearing on every re-sync. `record_violation`
//!      returns `Ok(true)` only on a fresh insert so the caller knows to
//!      fan out a WeChat alert exactly once per new violation.
//!   3. After dispatch, the caller marks the row with
//!      [`StorageState::mark_violation_notified`] to record the push timestamp.
//!
//! [`StorageState::count_violations_today`] is exposed for the daily-report
//! scheduler — it's an aggregate over the local-day window and intentionally
//! ignores uid filtering so the operator gets a system-wide tally.

use sqlx::Row;

use super::{StorageError, StorageState};

/// A pending violation row — only what the WeChat notifier needs to render a
/// push (id + uid + item identity + reason text).
#[derive(Debug, Clone)]
pub struct ViolationRow {
    pub id: i64,
    pub uid: String,
    pub item_id: String,
    pub item_title: String,
    pub violation_kind: String,
    pub reason_text: String,
    pub detected_at: i64,
}

impl StorageState {
    /// Insert a freshly-detected violation. Returns `Ok(true)` if a new row was
    /// created, `Ok(false)` if the UNIQUE (uid, item_id, violation_kind) tuple
    /// already exists (i.e. we've already alerted on this exact violation).
    ///
    /// `raw_metadata` is the JSON snapshot of upstream fields that triggered
    /// the detection — stored verbatim for post-hoc debugging when we tune the
    /// keyword set.
    pub async fn record_violation(
        &self,
        uid: &str,
        item_id: &str,
        item_title: &str,
        violation_kind: &str,
        reason_text: &str,
        raw_metadata: &str,
        detected_at_ms: i64,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            INSERT INTO account_violations (
                uid, item_id, item_title, violation_kind,
                reason_text, raw_metadata, detected_at, notified_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, 0)
            ON CONFLICT(uid, item_id, violation_kind) DO NOTHING
            "#,
        )
        .bind(uid)
        .bind(item_id)
        .bind(item_title)
        .bind(violation_kind)
        .bind(reason_text)
        .bind(raw_metadata)
        .bind(detected_at_ms)
        .execute(&self.xianyu)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Fetch rows that haven't been pushed yet (`notified_at = 0`). The caller
    /// is expected to push and then call `mark_violation_notified` per row.
    ///
    /// `limit` is enforced as i64 so a caller passing 0 / negative gets an
    /// empty Vec without surprises.
    pub async fn pending_violations(&self, limit: i64) -> Result<Vec<ViolationRow>, StorageError> {
        if limit <= 0 {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            r#"
            SELECT id, uid, item_id, item_title, violation_kind, reason_text, detected_at
            FROM account_violations
            WHERE notified_at = 0
            ORDER BY detected_at ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.xianyu)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ViolationRow {
                id: row.get::<i64, _>("id"),
                uid: row.get::<String, _>("uid"),
                item_id: row.get::<String, _>("item_id"),
                item_title: row.get::<String, _>("item_title"),
                violation_kind: row.get::<String, _>("violation_kind"),
                reason_text: row.get::<String, _>("reason_text"),
                detected_at: row.get::<i64, _>("detected_at"),
            })
            .collect())
    }

    /// Stamp a row as pushed. `ts_ms` is the millisecond epoch at which the
    /// dispatch succeeded. Subsequent `pending_violations` calls skip this row.
    pub async fn mark_violation_notified(&self, id: i64, ts_ms: i64) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            UPDATE account_violations
            SET notified_at = ?
            WHERE id = ?
            "#,
        )
        .bind(ts_ms)
        .bind(id)
        .execute(&self.xianyu)
        .await?;

        Ok(())
    }

    /// Count violations whose `detected_at` (ms epoch) falls on today's local
    /// date. Used by the daily-report scheduler — uid filter NOT applied.
    pub async fn count_violations_today(&self) -> Result<i64, StorageError> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM account_violations
            WHERE date(detected_at / 1000, 'unixepoch', 'localtime')
                = date('now', 'localtime')
            "#,
        )
        .fetch_one(&self.xianyu)
        .await?;

        Ok(count)
    }

    /// List violations for the admin UI. When `uid` is `Some`, scope to that
    /// account; otherwise return rows across all accounts. Newest-first.
    pub async fn list_violations(
        &self,
        uid: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ViolationRow>, StorageError> {
        let limit = if limit <= 0 { 100 } else { limit };

        let rows = if let Some(uid) = uid {
            sqlx::query(
                r#"
                SELECT id, uid, item_id, item_title, violation_kind, reason_text, detected_at
                FROM account_violations
                WHERE uid = ?
                ORDER BY detected_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(uid)
            .bind(limit)
            .fetch_all(&self.xianyu)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, uid, item_id, item_title, violation_kind, reason_text, detected_at
                FROM account_violations
                ORDER BY detected_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(limit)
            .fetch_all(&self.xianyu)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(|row| ViolationRow {
                id: row.get::<i64, _>("id"),
                uid: row.get::<String, _>("uid"),
                item_id: row.get::<String, _>("item_id"),
                item_title: row.get::<String, _>("item_title"),
                violation_kind: row.get::<String, _>("violation_kind"),
                reason_text: row.get::<String, _>("reason_text"),
                detected_at: row.get::<i64, _>("detected_at"),
            })
            .collect())
    }
}
