use axum::{
    extract::{Extension, Json, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};
use sqlx::{Row, SqlitePool};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::{
    ensure_item_belongs_to_uid, ensure_market_uid_access, load_location_or_default,
    success_message, success_ret, successful_upstream_h5_request, successful_upstream_wx_request,
    unix_millis_now, unix_seconds_now, upstream_h5_request, value_to_string, ItemRefPayload,
};

const COMMENT_CATEGORY_RULES: [(&str, &[&str]); 5] = [
    (
        "物流",
        &["物流", "快递", "发货", "到货", "配送", "送达", "签收"],
    ),
    ("包装", &["包装", "包裹", "盒子", "外观", "封装", "保护"]),
    (
        "商品",
        &[
            "宝贝", "商品", "质量", "成色", "描述", "做工", "材质", "功能", "使用",
        ],
    ),
    ("服务", &["回复", "服务", "态度", "客服", "沟通", "售后"]),
    ("价格", &["价格", "便宜", "划算", "性价比", "值", "优惠"]),
];

#[derive(Debug, Deserialize)]
pub(super) struct CommentPublishPayload {
    uid: String,
    item_id: String,
    content: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CommentDeletePayload {
    uid: String,
    item_id: String,
    comment_id: String,
}

fn normalize_comment_capture_content(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn comment_has_repeated_run(text: &str, min_run: usize) -> bool {
    let mut last = '\0';
    let mut run = 0_usize;

    for ch in text.chars() {
        if ch == last {
            run += 1;
            if run >= min_run {
                return true;
            }
        } else {
            last = ch;
            run = 1;
        }
    }

    false
}

fn score_capture_comment_content(content: &str) -> (f64, String, Map<String, Value>) {
    let text = normalize_comment_capture_content(content);
    let length = text.chars().count() as i64;
    let mut penalties = 0.0_f64;
    let mut bonuses = 0.0_f64;

    if length >= 6 {
        bonuses += 0.2;
    }
    if length >= 12 {
        bonuses += 0.2;
    }
    if (8..=60).contains(&length) {
        bonuses += 0.15;
    }
    if text.chars().any(|ch| "，。！？,.!".contains(ch)) {
        bonuses += 0.1;
    }
    if text
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fa5}').contains(&ch))
    {
        bonuses += 0.1;
    }

    if length <= 3 {
        penalties += 0.45;
    } else if length <= 5 {
        penalties += 0.2;
    }

    let lowered = text.to_lowercase();
    let spam_markers = [
        "加微信",
        "vx",
        "v信",
        "私聊",
        "联系我",
        "推广",
        "返现",
        "优惠券",
        "http://",
        "https://",
        "www.",
    ];
    if spam_markers
        .iter()
        .any(|marker| lowered.contains(&marker.to_lowercase()))
    {
        penalties += 0.45;
    }
    if comment_has_repeated_run(&text, 4) {
        penalties += 0.2;
    }
    let question_count = text.matches('?').count() + text.matches('？').count();
    if question_count >= 3 {
        penalties += 0.1;
    }

    let score = (0.4 + bonuses - penalties).clamp(0.0, 1.0);
    let score = (score * 10_000.0).round() / 10_000.0;
    let quality = if score >= 0.75 {
        "good"
    } else if score >= 0.45 {
        "review"
    } else {
        "bad"
    };

    let mut breakdown = Map::new();
    breakdown.insert("length".to_string(), Value::from(length));
    breakdown.insert(
        "bonuses".to_string(),
        Value::from((bonuses * 10_000.0).round() / 10_000.0),
    );
    breakdown.insert(
        "penalties".to_string(),
        Value::from((penalties * 10_000.0).round() / 10_000.0),
    );
    breakdown.insert("final".to_string(), Value::from(score));

    (score, quality.to_string(), breakdown)
}

fn comment_value_by_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn comment_pick_first_text(value: &Value, paths: &[&[&str]]) -> String {
    for path in paths {
        if let Some(found) = comment_value_by_path(value, path) {
            if found.is_array() || found.is_object() {
                continue;
            }
            if let Some(text) = value_to_string(found) {
                let normalized = text.trim();
                if !normalized.is_empty() {
                    return normalized.to_string();
                }
            }
        }
    }
    String::new()
}

fn comment_array_candidate(value: Option<&Value>) -> Option<Vec<Value>> {
    let array = value?.as_array()?;
    if array.iter().any(Value::is_object) {
        Some(array.clone())
    } else {
        None
    }
}

fn deep_find_comment_list(value: &Value) -> Option<Vec<Value>> {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                let lowered = key.to_ascii_lowercase();
                if matches!(
                    lowered.as_str(),
                    "commentlist" | "comments" | "resultlist" | "list" | "rows"
                ) {
                    if let Some(array) = comment_array_candidate(Some(nested)) {
                        return Some(array);
                    }
                }
                if let Some(found) = deep_find_comment_list(nested) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            if !items.is_empty() && items.iter().all(Value::is_object) {
                let hits = items
                    .iter()
                    .take(3)
                    .filter(|item| {
                        item.as_object().is_some_and(|object| {
                            object.keys().any(|key| {
                                matches!(
                                    key.to_ascii_lowercase().as_str(),
                                    "content" | "commentcontent" | "comment" | "text" | "commentid"
                                )
                            })
                        })
                    })
                    .count();
                if hits > 0 {
                    return Some(items.clone());
                }
            }
            for item in items {
                if let Some(found) = deep_find_comment_list(item) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_comment_rows(raw: &Value) -> Vec<Value> {
    let direct_paths: [&[&str]; 7] = [
        &["commentList"],
        &["comments"],
        &["resultList"],
        &["data", "commentList"],
        &["data", "comments"],
        &["data", "resultList"],
        &["data", "data", "commentList"],
    ];

    for path in direct_paths {
        if let Some(array) = comment_array_candidate(comment_value_by_path(raw, path)) {
            return array;
        }
    }

    deep_find_comment_list(raw).unwrap_or_default()
}

fn derive_comment_category(content: &str, raw_payload: &Value) -> String {
    let mut candidates = vec![normalize_comment_capture_content(content)];
    let payload_text = comment_pick_first_text(
        raw_payload,
        &[
            &["category"],
            &["categoryName"],
            &["scene"],
            &["tag"],
            &["item", "categoryName"],
        ],
    );
    if !payload_text.is_empty() {
        candidates.push(payload_text);
    }

    for text in candidates {
        let normalized = normalize_comment_capture_content(&text);
        if normalized.is_empty() {
            continue;
        }
        for (category, keywords) in COMMENT_CATEGORY_RULES {
            if keywords.iter().any(|keyword| normalized.contains(keyword)) {
                return category.to_string();
            }
        }
    }

    "其他".to_string()
}

fn build_source_comment_key(
    comment_id: &str,
    author_id: &str,
    author_nick: &str,
    published_at: &str,
    content: &str,
) -> String {
    if !comment_id.trim().is_empty() {
        return comment_id.trim().to_string();
    }

    let fallback = format!(
        "{}|{}|{}|{}",
        author_id.trim(),
        author_nick.trim(),
        published_at.trim(),
        normalize_comment_capture_content(content),
    );
    let mut hasher = Sha1::new();
    hasher.update(fallback.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn parse_comment_capture_items(raw: &Value) -> Vec<Value> {
    let snapshot_time = unix_seconds_now();
    let mut items = extract_comment_rows(raw)
        .into_iter()
        .filter_map(|row| {
            let content = normalize_comment_capture_content(&comment_pick_first_text(
                &row,
                &[
                    &["content"],
                    &["commentContent"],
                    &["comment"],
                    &["text"],
                    &["message"],
                    &["commentDO", "content"],
                    &["commentDO", "commentContent"],
                    &["messageDTO", "content"],
                ],
            ));
            if content.is_empty() {
                return None;
            }

            let comment_id = comment_pick_first_text(
                &row,
                &[
                    &["commentId"],
                    &["id"],
                    &["comment_id"],
                    &["commentDO", "commentId"],
                    &["commentDO", "id"],
                ],
            );
            let author_id = comment_pick_first_text(
                &row,
                &[
                    &["authorId"],
                    &["userId"],
                    &["buyerId"],
                    &["sellerId"],
                    &["author", "id"],
                    &["user", "id"],
                    &["userDO", "id"],
                ],
            );
            let author_nick = comment_pick_first_text(
                &row,
                &[
                    &["authorNick"],
                    &["userNick"],
                    &["buyerNick"],
                    &["sellerNick"],
                    &["nick"],
                    &["author", "nick"],
                    &["user", "nick"],
                    &["userDO", "nick"],
                ],
            );
            let published_at_text = comment_pick_first_text(
                &row,
                &[
                    &["publishTime"],
                    &["createTime"],
                    &["gmtCreate"],
                    &["commentTime"],
                    &["time"],
                    &["publishedAt"],
                    &["commentDO", "publishTime"],
                ],
            );
            let published_at = published_at_text
                .parse::<f64>()
                .unwrap_or(snapshot_time);
            let (score, quality, score_breakdown) = score_capture_comment_content(&content);
            let category = derive_comment_category(&content, &row);
            let source_comment_key = build_source_comment_key(
                &comment_id,
                &author_id,
                &author_nick,
                &published_at_text,
                &content,
            );
            Some(json!({
                "commentId": comment_id,
                "authorId": author_id,
                "authorNick": author_nick,
                "content": content,
                "publishedAt": published_at,
                "publishedAtText": if published_at_text.is_empty() { published_at.to_string() } else { published_at_text },
                "snapshot_time": snapshot_time,
                "score": score,
                "quality": quality,
                "category": category,
                "scoreBreakdown": score_breakdown,
                "sourceCommentKey": source_comment_key,
                "rawJson": row,
            }))
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        let left_score = left
            .get("score")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let right_score = right
            .get("score")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items
}

fn quality_counts(items: &[Value]) -> Value {
    let mut good = 0_usize;
    let mut review = 0_usize;
    let mut bad = 0_usize;

    for item in items {
        match item.get("quality").and_then(Value::as_str).unwrap_or("bad") {
            "good" => good += 1,
            "review" => review += 1,
            _ => bad += 1,
        }
    }

    json!({
        "good": good,
        "review": review,
        "bad": bad,
        "total": items.len(),
    })
}

pub(super) async fn item_comment_list(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    if let Ok(upstream) = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.comment.list"),
        "2.0",
        json!({
            "itemId": payload.item_id.clone(),
            "pageSize": 50,
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }
    let wx_item_id = payload
        .item_id
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(payload.item_id.clone()));
    if let Ok(upstream) = successful_upstream_wx_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.comment.list"),
        "1.0",
        json!({
            "utdid": "h5_client",
            "platform": "ios",
            "miniAppVersion": "0.3.78",
            "itemId": wx_item_id,
            "pageNumber": 1,
            "rowsPerPage": 50,
        }),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }
    let items = query_comments(state.storage().xianyu(), &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "items": items,
            "totalCount": items.len(),
        }
    })))
}

pub(super) async fn item_comment_capture(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    if let Ok(upstream) = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.comment.list"),
        "2.0",
        json!({
            "itemId": payload.item_id.clone(),
            "pageSize": 50,
        }),
        Vec::new(),
    )
    .await
    {
        let items = parse_comment_capture_items(&upstream);
        if !items.is_empty() {
            upsert_comment_snapshots(
                state.storage().xianyu(),
                &payload.uid,
                &payload.item_id,
                crate::sealed_protocol::sealed_str!("mtop.taobao.idle.comment.list"),
                "h5",
                &items,
            )
            .await
            .map_err(ResponseError::internal)?;
            replace_comments_snapshot(
                state.storage().xianyu(),
                &payload.uid,
                &payload.item_id,
                &items,
            )
            .await
            .map_err(ResponseError::internal)?;
            return Ok(ResponseJson(json!({
                "uid": payload.uid,
                "item_id": payload.item_id,
                "source_type": "h5",
                "source_api": crate::sealed_protocol::sealed_str!("mtop.taobao.idle.comment.list"),
                "quality_counts": quality_counts(&items),
                "items": items,
            })));
        }
    }

    let wx_item_id = payload
        .item_id
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(payload.item_id.clone()));
    if let Ok(upstream) = successful_upstream_wx_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.comment.list"),
        "1.0",
        json!({
            "utdid": "h5_client",
            "platform": "ios",
            "miniAppVersion": "0.3.78",
            "itemId": wx_item_id,
            "pageNumber": 1,
            "rowsPerPage": 50,
        }),
    )
    .await
    {
        let items = parse_comment_capture_items(&upstream);
        if !items.is_empty() {
            upsert_comment_snapshots(
                state.storage().xianyu(),
                &payload.uid,
                &payload.item_id,
                crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.comment.list"),
                "wx",
                &items,
            )
            .await
            .map_err(ResponseError::internal)?;
            replace_comments_snapshot(
                state.storage().xianyu(),
                &payload.uid,
                &payload.item_id,
                &items,
            )
            .await
            .map_err(ResponseError::internal)?;
            return Ok(ResponseJson(json!({
                "uid": payload.uid,
                "item_id": payload.item_id,
                "source_type": "wx",
                "source_api": crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.comment.list"),
                "quality_counts": quality_counts(&items),
                "items": items,
            })));
        }
    }

    let items = query_comments(state.storage().xianyu(), &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(json!({
        "uid": payload.uid,
        "item_id": payload.item_id,
        "source_type": "h5",
        "source_api": "simulated.item.comment.capture",
        "quality_counts": {
            "good": items.len(),
            "review": 0,
            "bad": 0,
        },
        "items": items,
    })))
}

pub(super) async fn item_comment_publish(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentPublishPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    if let Ok(upstream) = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.comment.publish"),
        "2.0",
        json!({
            "bizType": 0,
            "content": payload.content.trim(),
            "itemId": payload.item_id.clone(),
            "sellerId": "",
            "sellerNick": "",
            "title": "",
            "fishAdCode": location.fish_ad_code,
            "gps": format!("{},{}", location.latitude, location.longitude),
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }

    let wx_item_id = payload
        .item_id
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(payload.item_id.clone()));
    let mut title = String::new();
    let mut seller_id = Value::String(String::new());
    if let Ok(detail) = successful_upstream_wx_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.detail"),
        "1.0",
        json!({
            "utdid": "h5_client",
            "platform": "ios",
            "miniAppVersion": "0.3.78",
            "itemId": wx_item_id.clone(),
        }),
    )
    .await
    {
        let data = detail.get("data").cloned().unwrap_or(Value::Null);
        title = data
            .get("title")
            .or_else(|| {
                data.get(crate::sealed_protocol::sealed_str!("itemDO"))
                    .and_then(|item_do| item_do.get("title"))
            })
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        seller_id = data
            .get("sellerId")
            .cloned()
            .or_else(|| {
                data.get("trackParams")
                    .and_then(|track| track.get("sellerId"))
                    .cloned()
            })
            .unwrap_or_else(|| Value::String(String::new()));
    }
    if let Ok(upstream) = successful_upstream_wx_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.comment.publish"),
        "1.0",
        json!({
            "utdid": "h5_client",
            "platform": "ios",
            "miniAppVersion": "0.3.78",
            "bizType": "0",
            "content": payload.content.trim(),
            "itemId": wx_item_id,
            "sellerId": seller_id,
            "sellerNick": "",
            "title": title,
        }),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }

    let now = unix_seconds_now();
    let comment_id = format!("comment-{}", unix_millis_now());
    sqlx::query(
        r#"
        INSERT INTO item_comments (
            uid,
            item_id,
            comment_id,
            author_id,
            author_nick,
            content,
            published_at,
            deleted,
            created_at,
            updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
        "#,
    )
    .bind(&payload.uid)
    .bind(&payload.item_id)
    .bind(&comment_id)
    .bind(&payload.uid)
    .bind("我")
    .bind(payload.content.trim())
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "commentId": comment_id,
            "content": payload.content.trim(),
            "publishedAt": now,
        }
    })))
}

pub(super) async fn item_comment_delete(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentDeletePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let wx_item_id = payload
        .item_id
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(payload.item_id.clone()));
    let wx_comment_id = payload
        .comment_id
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(payload.comment_id.clone()));
    if let Ok(upstream) = successful_upstream_wx_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.comment.delete"),
        "1.0",
        json!({
            "utdid": "h5_client",
            "platform": "ios",
            "miniAppVersion": "0.3.78",
            "itemId": wx_item_id,
            "commentId": wx_comment_id,
        }),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }
    if let Ok(upstream) = upstream_h5_request(
        &state,
        &payload.uid,
        "com.taobao.idle.comment.delete",
        "2.0",
        json!({
            "itemId": payload.item_id.clone(),
            "commentId": payload.comment_id.clone(),
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }
    sqlx::query(
        "UPDATE item_comments SET deleted = 1, updated_at = ? WHERE uid = ? AND item_id = ? AND comment_id = ?",
    )
    .bind(unix_seconds_now())
    .bind(&payload.uid)
    .bind(&payload.item_id)
    .bind(&payload.comment_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(success_message("评论已删除")))
}

async fn query_comments(
    pool: &SqlitePool,
    uid: &str,
    item_id: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT comment_id, author_id, author_nick, content, published_at, created_at
        FROM item_comments
        WHERE uid = ? AND item_id = ? AND deleted = 0
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(uid)
    .bind(item_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            json!({
                "commentId": row.get::<String, _>("comment_id"),
                "authorId": row.get::<String, _>("author_id"),
                "authorNick": row.get::<String, _>("author_nick"),
                "content": row.get::<String, _>("content"),
                "publishedAt": row.get::<f64, _>("published_at"),
                "snapshot_time": row.get::<f64, _>("created_at"),
                "score": 0.95,
                "quality": "good",
            })
        })
        .collect())
}

async fn upsert_comment_snapshots(
    pool: &SqlitePool,
    uid: &str,
    item_id: &str,
    source_api: &str,
    source_type: &str,
    items: &[Value],
) -> Result<(), sqlx::Error> {
    let now = unix_seconds_now();

    for item in items {
        let content = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let content_hash = {
            let mut hasher = Sha1::new();
            hasher.update(content.as_bytes());
            format!("{:x}", hasher.finalize())
        };
        let raw_value = item
            .get("rawJson")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        let score_breakdown_value = item
            .get("scoreBreakdown")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        let raw_json = serde_json::to_string(&raw_value).unwrap_or_else(|_| "{}".to_string());
        let score_breakdown_json =
            serde_json::to_string(&score_breakdown_value).unwrap_or_else(|_| "{}".to_string());

        sqlx::query(
            r#"
            INSERT INTO item_comment_snapshots (
                uid, item_id, source_api, source_type, source_comment_key,
                comment_id, author_id, author_nick, content, content_hash,
                published_at, category, quality_score, quality_label, score_breakdown_json,
                raw_json, snapshot_time, accepted_for_txt, txt_exported_at, txt_export_path,
                txt_imported_at, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, '', 0, ?, ?)
            ON CONFLICT(uid, item_id, source_type, source_comment_key) DO UPDATE SET
                source_api = excluded.source_api,
                comment_id = excluded.comment_id,
                author_id = excluded.author_id,
                author_nick = excluded.author_nick,
                content = excluded.content,
                content_hash = excluded.content_hash,
                published_at = excluded.published_at,
                category = excluded.category,
                quality_score = excluded.quality_score,
                quality_label = excluded.quality_label,
                score_breakdown_json = excluded.score_breakdown_json,
                raw_json = excluded.raw_json,
                snapshot_time = excluded.snapshot_time,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(uid)
        .bind(item_id)
        .bind(source_api)
        .bind(source_type)
        .bind(
            item.get("sourceCommentKey")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(
            item.get("commentId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(
            item.get("authorId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(
            item.get("authorNick")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(&content)
        .bind(content_hash)
        .bind(
            item.get("publishedAtText")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(
            item.get("category")
                .and_then(Value::as_str)
                .unwrap_or("其他"),
        )
        .bind(
            item.get("score")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
        )
        .bind(item.get("quality").and_then(Value::as_str).unwrap_or("bad"))
        .bind(score_breakdown_json)
        .bind(raw_json)
        .bind(
            item.get("snapshot_time")
                .and_then(Value::as_f64)
                .unwrap_or(now),
        )
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn replace_comments_snapshot(
    pool: &SqlitePool,
    uid: &str,
    item_id: &str,
    items: &[Value],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM item_comments WHERE uid = ? AND item_id = ?")
        .bind(uid)
        .bind(item_id)
        .execute(pool)
        .await?;
    let now = unix_seconds_now();
    for item in items {
        sqlx::query(
            r#"
            INSERT INTO item_comments (
                uid,
                item_id,
                comment_id,
                author_id,
                author_nick,
                content,
                published_at,
                deleted,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
            "#,
        )
        .bind(uid)
        .bind(item_id)
        .bind(
            item.get("commentId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(
            item.get("authorId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(
            item.get("authorNick")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(
            item.get("content")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .bind(
            item.get("publishedAt")
                .and_then(Value::as_f64)
                .unwrap_or(now),
        )
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}
