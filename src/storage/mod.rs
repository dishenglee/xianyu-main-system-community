mod business;
mod comments;
mod delivery;
mod notifications;
pub(crate) mod operations;
pub mod publish_locations;
mod runtime_logs;
mod sqlite_busy;
mod violations;

use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use serde_json::{Map, Value};
use sha2::{Digest, Sha384};
use sqlx::{
    migrate::Migrator,
    query_scalar,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Pool, Row, Sqlite,
};

use crate::{config::AppConfig, models::runtime::DatabaseStatus};

static ACCOUNTS_MIGRATOR: Migrator = sqlx::migrate!("./migrations/accounts");
static XIANYU_MIGRATOR: Migrator = sqlx::migrate!("./migrations/xianyu");
static LIVE_STATE_MIGRATOR: Migrator = sqlx::migrate!("./migrations/live_state");
static NOTIFICATIONS_MIGRATOR: Migrator = sqlx::migrate!("./migrations/notifications");

pub(crate) use business::{
    is_global_scope_key, public_cookie_id, user_global_scope_key, GLOBAL_AI_SETTINGS_KEY,
};
pub(crate) use operations::{NewOperationJob, NewOperationJobItem, NewOperationLog};
pub(crate) use runtime_logs::RuntimeLogFilter;
pub use sqlite_busy::{
    begin_immediate_with_retry, is_sqlite_busy, SQLITE_BUSY_DELAYS, SQLITE_BUSY_MAX_ATTEMPTS,
};

#[derive(Debug, Clone)]
pub struct StorageState {
    accounts: Pool<Sqlite>,
    xianyu: Pool<Sqlite>,
    live_state: Pool<Sqlite>,
    notifications: Pool<Sqlite>,
    databases: Vec<DatabaseStatus>,
    startup_count: u64,
}

#[derive(Debug, Clone)]
pub struct LiveConnectionStateRecord {
    pub uid: String,
    pub status: String,
    pub last_error: String,
    pub last_connected_at: f64,
    pub last_disconnected_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone)]
pub struct LiveConnectionStateDetail {
    pub uid: String,
    pub status: String,
    pub last_error: String,
    pub reconnect_attempts: i64,
    pub last_connected_at: f64,
    pub last_disconnected_at: f64,
    pub updated_at: f64,
}

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Sqlx(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
    Conflict {
        message: String,
    },
    OwnerScopeMissing {
        account_uid: String,
    },
    DuplicateOneTimeCard {
        owner_scope_key: String,
        pool_tag: String,
    },
    BusyExhausted {
        operation: &'static str,
        attempts: usize,
    },
    InvalidDatabaseUrl {
        path: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "sqlite storage I/O error: {error}"),
            Self::Sqlx(error) => write!(f, "sqlite storage query error: {error}"),
            Self::Migration(error) => write!(f, "sqlite migration error: {error}"),
            Self::Conflict { message } => write!(f, "sqlite storage conflict: {message}"),
            Self::OwnerScopeMissing { account_uid } => {
                write!(
                    f,
                    "sqlite storage owner scope missing for account: {account_uid}"
                )
            }
            Self::DuplicateOneTimeCard {
                owner_scope_key,
                pool_tag,
            } => write!(
                f,
                "sqlite storage duplicate one-time card for scope {owner_scope_key} pool {pool_tag}"
            ),
            Self::BusyExhausted {
                operation,
                attempts,
            } => write!(
                f,
                "sqlite storage busy exhausted for operation {operation} after {attempts} attempts"
            ),
            Self::InvalidDatabaseUrl { path, message } => {
                write!(
                    f,
                    "failed to build sqlite connection options for {}: {}",
                    path.display(),
                    message
                )
            }
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for StorageError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

impl From<sqlx::migrate::MigrateError> for StorageError {
    fn from(value: sqlx::migrate::MigrateError) -> Self {
        Self::Migration(value)
    }
}

impl StorageState {
    pub async fn bootstrap(config: &AppConfig, started_at: &str) -> Result<Self, StorageError> {
        fs::create_dir_all(&config.sqlite.data_dir)?;

        let accounts_path = config.sqlite.accounts_db_path.clone();
        let xianyu_path = config.sqlite.xianyu_db_path.clone();
        let live_state_path = config.sqlite.live_state_db_path.clone();
        let notifications_path = config.sqlite.notifications_db_path.clone();
        let max_connections = config.sqlite.max_connections;

        let (accounts, accounts_migrations) =
            connect_and_migrate(&accounts_path, "migrations/accounts", max_connections).await?;
        let (xianyu, xianyu_migrations) =
            connect_and_migrate(&xianyu_path, "migrations/xianyu", max_connections).await?;
        let (live_state, live_state_migrations) =
            connect_and_migrate(&live_state_path, "migrations/live_state", max_connections).await?;
        let (notifications, notifications_migrations) = connect_and_migrate(
            &notifications_path,
            "migrations/notifications",
            max_connections,
        )
        .await?;

        let startup_count = record_startup(&accounts, &config.service_name, started_at).await?;
        let databases = vec![
            DatabaseStatus {
                name: "accounts".to_string(),
                path: accounts_path.display().to_string(),
                migrations: accounts_migrations,
            },
            DatabaseStatus {
                name: "xianyu".to_string(),
                path: xianyu_path.display().to_string(),
                migrations: xianyu_migrations,
            },
            DatabaseStatus {
                name: "live_state".to_string(),
                path: live_state_path.display().to_string(),
                migrations: live_state_migrations,
            },
            DatabaseStatus {
                name: "notifications".to_string(),
                path: notifications_path.display().to_string(),
                migrations: notifications_migrations,
            },
        ];

        Ok(Self {
            accounts,
            xianyu,
            live_state,
            notifications,
            databases,
            startup_count,
        })
    }

    pub fn databases(&self) -> &[DatabaseStatus] {
        &self.databases
    }

    pub fn startup_count(&self) -> u64 {
        self.startup_count
    }

    pub fn accounts(&self) -> &Pool<Sqlite> {
        &self.accounts
    }

    pub fn xianyu(&self) -> &Pool<Sqlite> {
        &self.xianyu
    }

    pub fn live_state(&self) -> &Pool<Sqlite> {
        &self.live_state
    }

    pub fn notifications(&self) -> &Pool<Sqlite> {
        &self.notifications
    }

    pub async fn record_login_audit(
        &self,
        username: &str,
        legacy: bool,
    ) -> Result<(), StorageError> {
        let token_kind = if legacy { "legacy" } else { "jwt" };

        sqlx::query(
            r#"
            INSERT INTO auth_login_audit (username, legacy, token_kind, issued_at)
            VALUES (?, ?, ?, unixepoch('subsec'))
            "#,
        )
        .bind(username)
        .bind(if legacy { 1_i64 } else { 0_i64 })
        .bind(token_kind)
        .execute(&self.accounts)
        .await?;

        Ok(())
    }

    pub async fn upsert_worker_state(
        &self,
        worker_name: &str,
        status: &str,
        tick_count: u64,
        last_error: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO service_worker_state (worker_name, status, tick_count, last_tick_at, last_error, updated_at)
            VALUES (?, ?, ?, ?, ?, unixepoch('subsec'))
            ON CONFLICT(worker_name) DO UPDATE SET
                status = excluded.status,
                tick_count = excluded.tick_count,
                last_tick_at = excluded.last_tick_at,
                last_error = excluded.last_error,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(worker_name)
        .bind(status)
        .bind(tick_count as i64)
        .bind(format!("{:?}", std::time::SystemTime::now()))
        .bind(last_error.unwrap_or(""))
        .execute(&self.accounts)
        .await?;

        Ok(())
    }

    pub async fn upsert_live_connection_state(
        &self,
        uid: &str,
        status: &str,
        reconnect_attempts: i64,
        last_error: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO live_connection_state (
                uid,
                status,
                last_connected_at,
                last_disconnected_at,
                last_error,
                reconnect_attempts,
                updated_at
            )
            VALUES (
                ?,
                ?,
                CASE WHEN ? = 'connected' THEN unixepoch('subsec') ELSE 0 END,
                CASE WHEN ? = 'disconnected' THEN unixepoch('subsec') ELSE 0 END,
                ?,
                ?,
                unixepoch('subsec')
            )
            ON CONFLICT(uid) DO UPDATE SET
                status = excluded.status,
                last_connected_at = CASE
                    WHEN excluded.status = 'connected' THEN excluded.last_connected_at
                    ELSE live_connection_state.last_connected_at
                END,
                last_disconnected_at = CASE
                    WHEN excluded.status = 'disconnected' THEN excluded.last_disconnected_at
                    ELSE live_connection_state.last_disconnected_at
                END,
                last_error = excluded.last_error,
                reconnect_attempts = excluded.reconnect_attempts,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(uid)
        .bind(status)
        .bind(status)
        .bind(status)
        .bind(last_error)
        .bind(reconnect_attempts)
        .execute(&self.live_state)
        .await?;

        Ok(())
    }

    pub async fn get_live_sync_cursor(&self, uid: &str) -> Result<(i64, i64), StorageError> {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT COALESCE(last_pts, 0), COALESCE(last_high_pts, 0) FROM live_connection_state WHERE uid = ?",
        )
        .bind(uid)
        .fetch_optional(&self.live_state)
        .await?;

        Ok(row.unwrap_or((0, 0)))
    }

    pub async fn update_live_sync_cursor(
        &self,
        uid: &str,
        last_pts: i64,
        last_high_pts: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO live_connection_state (uid, last_pts, last_high_pts)
            VALUES (?, ?, ?)
            ON CONFLICT(uid) DO UPDATE SET
                last_pts = excluded.last_pts,
                last_high_pts = excluded.last_high_pts
            "#,
        )
        .bind(uid)
        .bind(last_pts.max(0))
        .bind(last_high_pts.max(0))
        .execute(&self.live_state)
        .await?;

        Ok(())
    }

    pub async fn update_correlation_group_id(
        &self,
        uid: &str,
        correlation_group_id: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE live_connection_state SET correlation_group_id = ?, updated_at = unixepoch('subsec') WHERE uid = ?",
        )
        .bind(correlation_group_id)
        .bind(uid)
        .execute(&self.live_state)
        .await?;

        Ok(())
    }

    pub async fn get_correlation_group_id(&self, uid: &str) -> Result<String, StorageError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT COALESCE(correlation_group_id, '') FROM live_connection_state WHERE uid = ?",
        )
        .bind(uid)
        .fetch_optional(&self.live_state)
        .await?;

        Ok(row.map(|r| r.0).unwrap_or_default())
    }

    pub async fn list_live_connection_states(
        &self,
    ) -> Result<Vec<LiveConnectionStateRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT uid, status, last_error, last_connected_at, last_disconnected_at, updated_at
            FROM live_connection_state
            WHERE status != ''
            ORDER BY updated_at DESC, uid ASC
            "#,
        )
        .fetch_all(&self.live_state)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| LiveConnectionStateRecord {
                uid: row.get::<String, _>("uid"),
                status: row.get::<String, _>("status"),
                last_error: row.get::<String, _>("last_error"),
                last_connected_at: row.get::<f64, _>("last_connected_at"),
                last_disconnected_at: row.get::<f64, _>("last_disconnected_at"),
                updated_at: row.get::<f64, _>("updated_at"),
            })
            .collect())
    }

    pub async fn get_live_connection_state(
        &self,
        uid: &str,
    ) -> Result<Option<LiveConnectionStateDetail>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT uid, status, last_error, reconnect_attempts, last_connected_at, last_disconnected_at, updated_at
            FROM live_connection_state
            WHERE uid = ?
            LIMIT 1
            "#,
        )
        .bind(uid)
        .fetch_optional(&self.live_state)
        .await?;

        Ok(row.map(|row| LiveConnectionStateDetail {
            uid: row.get::<String, _>("uid"),
            status: row.get::<String, _>("status"),
            last_error: row.get::<String, _>("last_error"),
            reconnect_attempts: row.get::<i64, _>("reconnect_attempts"),
            last_connected_at: row.get::<f64, _>("last_connected_at"),
            last_disconnected_at: row.get::<f64, _>("last_disconnected_at"),
            updated_at: row.get::<f64, _>("updated_at"),
        }))
    }

    pub async fn upsert_message_live_state(
        &self,
        uid: &str,
        session_id: &str,
        session_type: i64,
        peer_uid: &str,
        peer_nick: &str,
        item_id: &str,
        item_title: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid,
                session_id,
                session_type,
                peer_uid,
                peer_nick,
                item_id,
                item_title,
                last_ws_event_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'), unixepoch('subsec'))
            ON CONFLICT(uid, session_id) DO UPDATE SET
                session_type = excluded.session_type,
                peer_uid = excluded.peer_uid,
                peer_nick = excluded.peer_nick,
                item_id = excluded.item_id,
                item_title = excluded.item_title,
                last_ws_event_at = excluded.last_ws_event_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(session_type)
        .bind(peer_uid)
        .bind(peer_nick)
        .bind(item_id)
        .bind(item_title)
        .execute(&self.live_state)
        .await?;

        Ok(())
    }

    pub async fn probe_databases(&self) -> Result<(), StorageError> {
        probe_pool(&self.accounts).await?;
        probe_pool(&self.xianyu).await?;
        probe_pool(&self.live_state).await?;
        probe_pool(&self.notifications).await?;
        Ok(())
    }
}

async fn connect_and_migrate(
    path: &Path,
    migrations_relative: &str,
    max_connections: u32,
) -> Result<(Pool<Sqlite>, u64), StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let connect_options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .map_err(|error| StorageError::InvalidDatabaseUrl {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_millis(50));

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(connect_options)
        .await?;

    let migrator = match migrations_relative {
        "migrations/accounts" => &ACCOUNTS_MIGRATOR,
        "migrations/xianyu" => &XIANYU_MIGRATOR,
        "migrations/live_state" => &LIVE_STATE_MIGRATOR,
        "migrations/notifications" => &NOTIFICATIONS_MIGRATOR,
        _ => {
            return Err(StorageError::InvalidDatabaseUrl {
                path: path.to_path_buf(),
                message: format!("unknown migration set: {migrations_relative}"),
            });
        }
    };
    if migrations_relative == "migrations/accounts" {
        run_accounts_migrations(&pool).await?;
        ensure_accounts_compat_schema(&pool).await?;
    } else {
        if migrations_relative == "migrations/live_state" {
            reconcile_legacy_live_state_migration_checksum(&pool).await?;
        }
        migrator.run(&pool).await?;
    }

    if migrations_relative == "migrations/xianyu" {
        ensure_comment_task_schema(&pool).await?;
    }

    if migrations_relative == "migrations/notifications" {
        ensure_notifications_tenant_schema(&pool).await?;
    }

    let applied = query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await?
        .max(0) as u64;

    Ok((pool, applied))
}

// Migration 0005 briefly shipped with a comment-only MTOP reference. Existing
// databases carry that historical checksum, while the release artifact must
// keep the migration text redacted. Reconcile only that known checksum before
// SQLx validates the migration set; no schema operation is replayed.
async fn reconcile_legacy_live_state_migration_checksum(
    pool: &Pool<Sqlite>,
) -> Result<(), StorageError> {
    const LEGACY_CHECKSUM: [u8; 48] = [
        0xA2, 0xE7, 0x6F, 0x6D, 0x6B, 0x98, 0xBB, 0xCA, 0x15, 0x2D, 0x2C, 0xE6, 0x86, 0x4C, 0x59,
        0x26, 0xD2, 0xE7, 0xAC, 0x4B, 0xE3, 0x6F, 0x31, 0x6F, 0xB9, 0x0C, 0x2B, 0xD4, 0xB4, 0x07,
        0xFB, 0x6C, 0xD2, 0x75, 0xA3, 0x33, 0x00, 0x2D, 0x9E, 0xDD, 0xA1, 0xCA, 0x61, 0xD5, 0xB2,
        0x6D, 0x6D, 0x22,
    ];

    let table_exists = query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await?;
    if table_exists == 0 {
        return Ok(());
    }

    let checksum = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT checksum FROM _sqlx_migrations WHERE version = 5 AND success = 1",
    )
    .fetch_optional(pool)
    .await?;
    if checksum.as_deref() != Some(LEGACY_CHECKSUM.as_slice()) {
        return Ok(());
    }

    let current_checksum = Sha384::digest(include_bytes!(
        "../../migrations/live_state/0005_upstream_message_id.sql"
    ));
    sqlx::query(
        "UPDATE _sqlx_migrations SET checksum = ? WHERE version = 5 AND success = 1 AND checksum = ?",
    )
    .bind(current_checksum.as_slice())
    .bind(LEGACY_CHECKSUM.as_slice())
    .execute(pool)
    .await?;
    Ok(())
}

async fn run_accounts_migrations(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
    const COMPAT_CUTOFF: i64 = 24;

    // Migration 0024 installs a trigger that needs the compatibility columns.
    // Only databases that have not reached 0024 need the staged migration run.
    // Revalidating an already-upgraded database against a truncated migrator is
    // both unnecessary and fragile when historical migration files change.
    if accounts_migration_requires_compat(pool, COMPAT_CUTOFF).await? {
        let pre_compat = Migrator {
            migrations: Cow::Owned(
                ACCOUNTS_MIGRATOR
                    .iter()
                    .filter(|migration| migration.version < COMPAT_CUTOFF)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: true,
            locking: ACCOUNTS_MIGRATOR.locking,
            no_tx: ACCOUNTS_MIGRATOR.no_tx,
        };
        pre_compat.run(pool).await?;
        ensure_accounts_migration_prerequisites(pool).await?;
    }
    ACCOUNTS_MIGRATOR.run(pool).await?;
    Ok(())
}

async fn accounts_migration_requires_compat(
    pool: &Pool<Sqlite>,
    compat_cutoff: i64,
) -> Result<bool, StorageError> {
    let migration_table_exists = query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await?;
    if migration_table_exists == 0 {
        return Ok(true);
    }

    let latest_applied = query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
    )
    .fetch_one(pool)
    .await?;
    Ok(latest_applied < compat_cutoff)
}

async fn ensure_accounts_compat_schema(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
    ensure_accounts_migration_prerequisites(pool).await?;
    ensure_column(
        pool,
        "accounts",
        "bound_at",
        "ALTER TABLE accounts ADD COLUMN bound_at REAL NOT NULL DEFAULT 0",
    )
    .await?;
    sqlx::query(
        r#"
        UPDATE accounts
        SET bound_at = CASE
            WHEN COALESCE(owner_username, '') <> '' AND updated_at > 0 THEN updated_at
            WHEN created_at > 0 THEN created_at
            ELSE strftime('%s', 'now')
        END
        WHERE bound_at <= 0
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_accounts_owner_bound
        ON accounts (owner_username, bound_at DESC, uid ASC)
        "#,
    )
    .execute(pool)
    .await?;
    finalize_delivery_configuration_integrity(pool).await?;
    Ok(())
}

async fn ensure_accounts_migration_prerequisites(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
    ensure_column(
        pool,
        "cards",
        "reusable",
        "ALTER TABLE cards ADD COLUMN reusable INTEGER DEFAULT 0",
    )
    .await?;
    ensure_column(
        pool,
        "delivery_rules",
        "card_pool_scope",
        "ALTER TABLE delivery_rules ADD COLUMN card_pool_scope TEXT DEFAULT 'account'",
    )
    .await?;
    Ok(())
}

// SQLx installs the deferred 0024 trigger first; compatibility columns must
// exist before its supporting indexes and one-shot marker are finalized.
async fn finalize_delivery_configuration_integrity(
    pool: &Pool<Sqlite>,
) -> Result<(), StorageError> {
    for statement in [
        r#"
        CREATE INDEX IF NOT EXISTS idx_cards_cookie_tag_reusable_used
        ON cards (cookie_id, card_pool_tag, reusable, used, created_at DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_cards_delivery_reservation
        ON cards (cookie_id, card_pool_tag, reusable, used, reserved_run_id, created_at, id)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_cards_delivery_normalized_content
        ON cards (cookie_id, card_pool_tag, reusable, TRIM(card_content))
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_delivery_runs_uid_rule_status_updated
        ON delivery_runs (uid, rule_id, status, updated_at DESC, id DESC)
        "#,
        r#"
        CREATE INDEX IF NOT EXISTS idx_delivery_runs_updated_id_uid
        ON delivery_runs (updated_at DESC, id DESC, uid)
        "#,
    ] {
        sqlx::query(statement).execute(pool).await?;
    }
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO delivery_configuration_integrity_runs (id, requested_at)
        VALUES (1, unixepoch())
        "#,
    )
    .execute(pool)
    .await?;
    reconcile_auto_ship_flags_with_configurations(pool).await?;
    Ok(())
}

/// Turn off account auto-ship flags when the account has no delivery configurations left.
/// Historical saves/deletes only toggled the flag one way, leaving orphan "ON" states.
async fn reconcile_auto_ship_flags_with_configurations(
    pool: &Pool<Sqlite>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        UPDATE ai_reply_settings
        SET auto_ship_enabled = 0,
            updated_at = CAST(strftime('%s','now') AS INTEGER) * 1000
        WHERE auto_ship_enabled = 1
          AND cookie_id NOT IN (
              SELECT DISTINCT cookie_id FROM delivery_rules WHERE TRIM(cookie_id) <> ''
          )
        "#,
    )
    .execute(pool)
    .await?;

    let rows = sqlx::query(
        r#"
        SELECT uid, extra
        FROM accounts
        WHERE json_valid(extra)
          AND json_extract(extra, '$.autoShip') = 1
          AND uid NOT IN (
              SELECT DISTINCT cookie_id FROM delivery_rules WHERE TRIM(cookie_id) <> ''
          )
        "#,
    )
    .fetch_all(pool)
    .await?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    for row in rows {
        let uid: String = row.get("uid");
        let raw: String = row.get("extra");
        let mut extra = serde_json::from_str::<Map<String, Value>>(&raw).unwrap_or_default();
        extra.insert("autoShip".to_string(), Value::Bool(false));
        let serialized = serde_json::to_string(&extra).map_err(|error| StorageError::Conflict {
            message: format!("清理账号自动发货开关失败: {error}"),
        })?;
        sqlx::query("UPDATE accounts SET extra = ?, updated_at = ? WHERE uid = ?")
            .bind(serialized)
            .bind(now)
            .bind(uid)
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn ensure_notifications_tenant_schema(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
    ensure_column(
        pool,
        "notification_channels",
        "owner_username",
        "ALTER TABLE notification_channels ADD COLUMN owner_username TEXT NOT NULL DEFAULT ''",
    )
    .await?;
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_notification_channels_owner_updated
        ON notification_channels (owner_username, updated_at DESC, id DESC)
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_column(
    pool: &Pool<Sqlite>,
    table: &str,
    column: &str,
    alter_sql: &str,
) -> Result<(), StorageError> {
    let pragma = format!("PRAGMA table_info({table})");
    let columns: Vec<String> = sqlx::query(&pragma)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    if !columns.iter().any(|name| name == column) {
        sqlx::query(alter_sql).execute(pool).await?;
    }
    Ok(())
}

async fn record_startup(
    accounts: &Pool<Sqlite>,
    service_name: &str,
    started_at: &str,
) -> Result<u64, StorageError> {
    sqlx::query(
        r#"
        INSERT INTO service_runtime_state (service_name, version, started_at, startup_count, last_ready_at)
        VALUES (?, ?, ?, 1, ?)
        ON CONFLICT(service_name) DO UPDATE SET
            version = excluded.version,
            started_at = excluded.started_at,
            startup_count = service_runtime_state.startup_count + 1,
            last_ready_at = excluded.last_ready_at
        "#,
    )
    .bind(service_name)
    .bind(env!("CARGO_PKG_VERSION"))
    .bind(started_at)
    .bind(started_at)
    .execute(accounts)
    .await?;

    let startup_count = query_scalar::<_, i64>(
        "SELECT startup_count FROM service_runtime_state WHERE service_name = ?",
    )
    .bind(service_name)
    .fetch_one(accounts)
    .await?
    .max(0) as u64;

    Ok(startup_count)
}

async fn probe_pool(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
    query_scalar::<_, i64>("SELECT 1").fetch_one(pool).await?;
    Ok(())
}

async fn ensure_comment_task_schema(pool: &Pool<Sqlite>) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS comment_task_accounts (
            task_id TEXT NOT NULL,
            account_uid TEXT NOT NULL,
            position INTEGER NOT NULL DEFAULT 0,
            created_at REAL NOT NULL DEFAULT 0,
            PRIMARY KEY (task_id, account_uid)
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_comment_task_accounts_task_position ON comment_task_accounts(task_id, position)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS comment_task_progress (
            task_id TEXT PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'draft',
            total_targets INTEGER NOT NULL DEFAULT 0,
            completed_targets INTEGER NOT NULL DEFAULT 0,
            successful_targets INTEGER NOT NULL DEFAULT 0,
            failed_targets INTEGER NOT NULL DEFAULT 0,
            skipped_targets INTEGER NOT NULL DEFAULT 0,
            pending_targets INTEGER NOT NULL DEFAULT 0,
            percent REAL NOT NULL DEFAULT 0,
            current_target_id TEXT DEFAULT '',
            current_account_uid TEXT DEFAULT '',
            last_message TEXT DEFAULT '',
            updated_at REAL NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await?;

    let task_log_columns = sqlx::query("PRAGMA table_info(comment_task_logs)")
        .fetch_all(pool)
        .await?;
    let has_log_account_uid = task_log_columns
        .iter()
        .any(|row| row.try_get::<String, _>("name").ok().as_deref() == Some("account_uid"));
    let has_log_item_id = task_log_columns
        .iter()
        .any(|row| row.try_get::<String, _>("name").ok().as_deref() == Some("item_id"));
    if !has_log_account_uid {
        sqlx::query(
            "ALTER TABLE comment_task_logs ADD COLUMN account_uid TEXT NOT NULL DEFAULT ''",
        )
        .execute(pool)
        .await?;
    }
    if !has_log_item_id {
        sqlx::query("ALTER TABLE comment_task_logs ADD COLUMN item_id TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }

    let task_target_columns = sqlx::query("PRAGMA table_info(comment_task_targets)")
        .fetch_all(pool)
        .await?;
    let has_commented_account_uid = task_target_columns.iter().any(|row| {
        row.try_get::<String, _>("name").ok().as_deref() == Some("commented_account_uid")
    });
    if !has_commented_account_uid {
        sqlx::query(
            "ALTER TABLE comment_task_targets ADD COLUMN commented_account_uid TEXT NOT NULL DEFAULT ''",
        )
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_comment_task_targets_task_status ON comment_task_targets(task_id, status, position)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_comment_tasks_status_transition ON comment_tasks(status, last_transition_at DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_comment_delete_jobs_account_item ON comment_delete_jobs(account_uid, item_id, status, id DESC)",
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accounts_migration_compat_is_skipped_after_delivery_integrity_migration() {
        assert!(
            ACCOUNTS_MIGRATOR
                .iter()
                .any(|migration| migration.version == 10),
            "embedded accounts migrator must include migration 0010"
        );
        assert!(
            LIVE_STATE_MIGRATOR
                .iter()
                .any(|migration| migration.version == 10),
            "embedded live-state migrator must include migration 0010"
        );

        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();

        assert!(accounts_migration_requires_compat(&pool, 24).await.unwrap());

        sqlx::query(
            "CREATE TABLE _sqlx_migrations (version INTEGER PRIMARY KEY, success BOOLEAN NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO _sqlx_migrations (version, success) VALUES (23, 1)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(accounts_migration_requires_compat(&pool, 24).await.unwrap());

        sqlx::query("INSERT INTO _sqlx_migrations (version, success) VALUES (24, 1)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(!accounts_migration_requires_compat(&pool, 24).await.unwrap());
    }
}

pub(super) fn parse_json_map(raw: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

pub(super) fn parse_json_vec_strings(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

pub(super) fn parse_json_vec_i64(raw: &str) -> Vec<i64> {
    serde_json::from_str::<Vec<i64>>(raw).unwrap_or_default()
}

pub(super) fn unix_seconds_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs_f64()
}

pub(super) fn unix_millis_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}
