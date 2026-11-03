// One-click "sync items for ALL active accounts" handler.
//
// Replaces the previous frontend pattern of looping over every account and
// firing one `/api/items/sync` request per account. Backend now fans out the
// per-account `sync_items_to_db` work itself with bounded concurrency, then
// returns a single summary with per-account outcome so the UI can show
// "X/Y synced" without N HTTP round-trips.

use axum::{
    extract::{Extension, State},
    Json as ResponseJson,
};
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    routes::tenant::owner_username_filter,
    state::AppState,
};

use super::items_sync::sync_items_to_db;

// How many accounts we sync upstream in parallel. Each per-account sync hits
// the H5 management.query endpoint with pagination, plus coin + SOLD_OUT
// endpoints, so we deliberately keep this small to avoid rate-limit churn.
const SYNC_ALL_CONCURRENCY: usize = 4;

/// POST /api/items/sync_all
///
/// Body: none (active accounts are picked up automatically — same filter as
/// `GET /api/accounts?status=active` and `GET /api/accounts/health`).
///
/// Response:
/// ```json
/// {
///   "status": "ok" | "error",
///   "total": <n>,            // number of accounts attempted
///   "success_count": <n>,
///   "fail_count": <n>,
///   "total_items": <n>,      // sum of items persisted across all accounts
///   "accounts": [
///     { "uid": "...", "ok": true, "item_count": 42, "synced_at": 1.0, "detail": "..." },
///     ...
///   ]
/// }
/// ```
pub(super) async fn sync_all_items(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let owner = owner_username_filter(&state, &identity);
    let rows = if let Some(owner) = owner {
        sqlx::query(
            "SELECT uid FROM accounts WHERE status = 'active' AND owner_username = ? ORDER BY uid",
        )
        .bind(owner)
        .fetch_all(state.storage().accounts())
        .await
        .map_err(ResponseError::internal)?
    } else {
        sqlx::query("SELECT uid FROM accounts WHERE status = 'active' ORDER BY uid")
            .fetch_all(state.storage().accounts())
            .await
            .map_err(ResponseError::internal)?
    };
    let uids: Vec<String> = rows
        .into_iter()
        .map(|row| row.get::<String, _>("uid"))
        .collect();
    let total = uids.len();

    // Bounded fan-out: at most SYNC_ALL_CONCURRENCY upstream syncs in flight.
    // `state` is `Clone`, so each task gets its own handle.
    let state_ref = &state;
    let results: Vec<Value> = stream::iter(uids.into_iter())
        .map(|uid| async move {
            match sync_items_to_db(state_ref, &uid).await {
                Ok(outcome) => json!({
                    "uid": uid,
                    "ok": outcome.ok,
                    "item_count": outcome.total,
                    "synced_at": outcome.synced_at,
                    "detail": outcome.detail,
                }),
                Err(err) => json!({
                    "uid": uid,
                    "ok": false,
                    "item_count": 0,
                    "synced_at": 0.0,
                    "detail": err,
                }),
            }
        })
        .buffer_unordered(SYNC_ALL_CONCURRENCY)
        .collect()
        .await;

    let mut success_count = 0_usize;
    let mut fail_count = 0_usize;
    let mut total_items = 0_i64;
    for entry in &results {
        let ok = entry.get("ok").and_then(Value::as_bool).unwrap_or(false);
        if ok {
            success_count += 1;
        } else {
            fail_count += 1;
        }
        total_items += entry.get("item_count").and_then(Value::as_i64).unwrap_or(0);
    }

    tracing::info!(
        total = total,
        success_count = success_count,
        fail_count = fail_count,
        total_items = total_items,
        "sync_all_items finished"
    );

    Ok(ResponseJson(json!({
        "status": if fail_count == 0 { "ok" } else { "error" },
        "total": total,
        "success_count": success_count,
        "fail_count": fail_count,
        "total_items": total_items,
        "accounts": results,
    })))
}
