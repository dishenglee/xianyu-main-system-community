use std::time::Duration;

use sqlx::{Pool, Sqlite, Transaction};

use super::StorageError;

/// Backoff delays between BEGIN IMMEDIATE attempts after the first failure.
pub const SQLITE_BUSY_DELAYS: [Duration; 4] = [
    Duration::from_millis(10),
    Duration::from_millis(25),
    Duration::from_millis(50),
    Duration::from_millis(100),
];

/// Total attempts = first try + one per delay entry.
pub const SQLITE_BUSY_MAX_ATTEMPTS: usize = 1 + SQLITE_BUSY_DELAYS.len();

pub fn is_sqlite_busy(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database) => {
            if let Some(code) = database.code() {
                let code = code.to_string();
                if code == "5"
                    || code == "6"
                    || code == "SQLITE_BUSY"
                    || code == "SQLITE_LOCKED"
                    || code.starts_with("5")
                    || code.starts_with("6")
                {
                    return true;
                }
            }
            let message = database.message().to_ascii_lowercase();
            message.contains("database is locked")
                || message.contains("database is busy")
                || message.contains("sqlite_busy")
                || message.contains("sqlite_locked")
        }
        _ => false,
    }
}

pub async fn begin_immediate_with_retry<'a>(
    pool: &'a Pool<Sqlite>,
    operation: &'static str,
) -> Result<Transaction<'a, Sqlite>, StorageError> {
    let mut attempt = 0_usize;
    loop {
        attempt += 1;
        match pool.begin_with("BEGIN IMMEDIATE").await {
            Ok(tx) => return Ok(tx),
            Err(error) if is_sqlite_busy(&error) && attempt < SQLITE_BUSY_MAX_ATTEMPTS => {
                let delay = SQLITE_BUSY_DELAYS[attempt - 1];
                tokio::time::sleep(delay).await;
            }
            Err(error) if is_sqlite_busy(&error) => {
                return Err(StorageError::BusyExhausted {
                    operation,
                    attempts: SQLITE_BUSY_MAX_ATTEMPTS,
                });
            }
            Err(error) => return Err(StorageError::Sqlx(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_max_attempts_is_five() {
        assert_eq!(SQLITE_BUSY_MAX_ATTEMPTS, 5);
    }
}
