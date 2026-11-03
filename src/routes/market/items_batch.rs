// Batch item-delete handler.
//
// Upstream Xianyu has NO batch-delete API (only single
// `com.taobao.idle.item.delete`, see docs/xianyu_api_catalog.md). The frontend
// previously fanned out N parallel `POST /api/item/delete` requests for each
// batch-delete operation; this endpoint replaces that with ONE backend call
// that fans out the per-item upstream deletes with bounded concurrency and
// returns a single summary listing each item's outcome.
//
// Pattern mirrors `items_sync_all.rs` — `stream::iter().buffer_unordered(N)`
// for capped parallelism + state cloning across tasks.

use axum::{
    extract::{Extension, Json, State},
    Json as ResponseJson,
};
use futures_util::{stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    routes::tenant::{ensure_account_uids_accessible, owner_username_filter},
    state::AppState,
};

use super::{ensure_item_belongs_to_uid, success_ret, successful_upstream_h5_request};

// Upper bound on in-flight upstream deletes. Each call hits the same H5
// gateway; keep low to avoid risk-control spikes.
const BATCH_DELETE_CONCURRENCY: usize = 5;

#[derive(Debug, Deserialize)]
pub(super) struct BatchDeletionEntry {
    pub(super) uid: String,
    pub(super) item_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum BatchDeletePayload {
    // Single-account form: `{ "uid": "...", "item_ids": [...] }`
    SingleAccount { uid: String, item_ids: Vec<String> },
    // Cross-account form: `{ "deletions": [{ "uid": "...", "item_id": "..." }, ...] }`
    CrossAccount { deletions: Vec<BatchDeletionEntry> },
}

/// POST /api/items/batch_delete
///
/// Body (one of):
/// ```json
/// { "uid": "...", "item_ids": ["1001", "1002"] }
/// { "deletions": [{"uid": "u1", "item_id": "1001"}, {"uid": "u2", "item_id": "9001"}] }
/// ```
///
/// Response:
/// ```json
/// {
///   "ret": ["SUCCESS::调用成功"],
///   "total": 12,
///   "success_count": 10,
///   "fail_count": 2,
///   "results": [
///     {"uid":"u1","itemId":"1001","ok":true,"message":"删除成功"},
///     {"uid":"u1","itemId":"1002","ok":false,"message":"FAIL_BIZ_..."},
///     ...
///   ]
/// }
/// ```
pub(super) async fn item_batch_delete(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<BatchDeletePayload>,
) -> ApiResult {
    let targets: Vec<BatchDeletionEntry> = match payload {
        BatchDeletePayload::SingleAccount { uid, item_ids } => item_ids
            .into_iter()
            .filter(|id| !id.trim().is_empty())
            .map(|item_id| BatchDeletionEntry {
                uid: uid.clone(),
                item_id,
            })
            .collect(),
        BatchDeletePayload::CrossAccount { deletions } => deletions
            .into_iter()
            .filter(|entry| !entry.uid.trim().is_empty() && !entry.item_id.trim().is_empty())
            .collect(),
    };

    if targets.is_empty() {
        return Err(ResponseError::bad_request("item_ids 不能为空"));
    }
    let mut uids = targets
        .iter()
        .map(|entry| entry.uid.trim().to_string())
        .collect::<Vec<_>>();
    uids.sort();
    uids.dedup();
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uids_accessible(state.storage().accounts(), owner.as_deref(), &uids).await?;

    let total = targets.len();
    let state_ref = &state;

    let results: Vec<Value> = stream::iter(targets.into_iter())
        .map(|entry| async move { delete_one(state_ref, &entry.uid, &entry.item_id).await })
        .buffer_unordered(BATCH_DELETE_CONCURRENCY)
        .collect()
        .await;

    let mut success_count = 0_usize;
    let mut fail_count = 0_usize;
    for entry in &results {
        if entry.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            success_count += 1;
        } else {
            fail_count += 1;
        }
    }

    tracing::info!(
        total = total,
        success_count = success_count,
        fail_count = fail_count,
        "item_batch_delete finished"
    );

    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "total": total,
        "success_count": success_count,
        "fail_count": fail_count,
        "results": results,
    })))
}

// Delete a single (uid, item_id) pair: hit upstream first, then drop the
// local row (best-effort regardless of upstream outcome — the single-delete
// handler in `items.rs` does the same thing).
async fn delete_one(state: &AppState, uid: &str, item_id: &str) -> Value {
    if let Err(error) = ensure_item_belongs_to_uid(state, uid, item_id).await {
        return json!({
            "uid": uid,
            "itemId": item_id,
            "ok": false,
            "message": error.detail(),
        });
    }

    let upstream_result = successful_upstream_h5_request(
        state,
        uid,
        "com.taobao.idle.item.delete",
        "1.1",
        json!({
            "itemId": item_id,
        }),
        Vec::new(),
    )
    .await;

    let (ok, message) = match &upstream_result {
        Ok(_) => (true, "删除成功".to_string()),
        Err(err) => (false, clean_upstream_error(err)),
    };

    let _ = sqlx::query("DELETE FROM account_items WHERE uid = ? AND item_id = ?")
        .bind(uid)
        .bind(item_id)
        .execute(state.storage().xianyu())
        .await;

    json!({
        "uid": uid,
        "itemId": item_id,
        "ok": ok,
        "message": message,
    })
}

// Mirror frontend's `cleanMtopErr`: strip the "non-success upstream ret: "
// prefix so the per-item `message` field reads like a real upstream code.
fn clean_upstream_error(raw: &str) -> String {
    raw.strip_prefix("non-success upstream ret: ")
        .unwrap_or(raw)
        .to_string()
}
