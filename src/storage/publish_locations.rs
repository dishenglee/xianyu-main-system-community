use sqlx::Row;
use sqlx::{Pool, Sqlite};

use super::{unix_millis_now, StorageError, StorageState};

/// Per-account publish location override.
///
/// Mirrors the `publish_locations` SQLite table 1:1. Strings are non-null with
/// empty defaults so callers can populate just the fields they care about (the
/// minimum useful row is `fish_ad_code` + `gps`).
#[derive(Debug, Clone)]
pub struct PublishLocationRecord {
    pub uid: String,
    pub fish_ad_code: String,
    pub gps: String,
    pub latitude: String,
    pub longitude: String,
    pub name: String,
    pub address: String,
    pub province: String,
    pub city: String,
    pub district: String,
    pub updated_at: i64,
}

impl StorageState {
    pub async fn get_publish_location(
        &self,
        uid: &str,
    ) -> Result<Option<PublishLocationRecord>, StorageError> {
        self::get_publish_location(self.accounts(), uid).await
    }

    pub async fn set_publish_location(
        &self,
        record: &PublishLocationRecord,
    ) -> Result<(), StorageError> {
        let now = unix_millis_now();
        sqlx::query(
            r#"
            INSERT INTO publish_locations (
                uid,
                fish_ad_code,
                gps,
                latitude,
                longitude,
                name,
                address,
                province,
                city,
                district,
                updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(uid) DO UPDATE SET
                fish_ad_code = excluded.fish_ad_code,
                gps = excluded.gps,
                latitude = excluded.latitude,
                longitude = excluded.longitude,
                name = excluded.name,
                address = excluded.address,
                province = excluded.province,
                city = excluded.city,
                district = excluded.district,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&record.uid)
        .bind(&record.fish_ad_code)
        .bind(&record.gps)
        .bind(&record.latitude)
        .bind(&record.longitude)
        .bind(&record.name)
        .bind(&record.address)
        .bind(&record.province)
        .bind(&record.city)
        .bind(&record.district)
        .bind(now)
        .execute(self.accounts())
        .await?;
        Ok(())
    }

    pub async fn delete_publish_location(&self, uid: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM publish_locations WHERE uid = ?")
            .bind(uid)
            .execute(self.accounts())
            .await?;
        Ok(())
    }
}

/// Free function variant so non-`AppState` callers (e.g. `xianyu_client`, which
/// only holds the accounts `Pool`) can read without holding a `StorageState`.
pub async fn get_publish_location(
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<Option<PublishLocationRecord>, StorageError> {
    let row = sqlx::query(
        r#"
        SELECT uid, fish_ad_code, gps, latitude, longitude, name, address, province, city, district, updated_at
        FROM publish_locations
        WHERE uid = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .fetch_optional(accounts)
    .await?;

    Ok(row.map(|row| PublishLocationRecord {
        uid: row.get::<String, _>("uid"),
        fish_ad_code: row.get::<String, _>("fish_ad_code"),
        gps: row.get::<String, _>("gps"),
        latitude: row.get::<String, _>("latitude"),
        longitude: row.get::<String, _>("longitude"),
        name: row.get::<String, _>("name"),
        address: row.get::<String, _>("address"),
        province: row.get::<String, _>("province"),
        city: row.get::<String, _>("city"),
        district: row.get::<String, _>("district"),
        updated_at: row.get::<i64, _>("updated_at"),
    }))
}
