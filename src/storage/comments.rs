use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value};
use sha1::{Digest, Sha1};
use sqlx::{sqlite::SqliteRow, Pool, QueryBuilder, Row, Sqlite};

use crate::models::comment::{
    CommentCandidate, CommentCandidateSummary, CommentExportSummary, CommentImportSummary,
    CommentLibraryItem, CommentTarget,
};

use super::{parse_json_map, unix_millis_now, unix_seconds_now, StorageError, StorageState};

impl StorageState {
    pub async fn upsert_comment_targets(
        &self,
        owner_username: &str,
        targets: &[CommentTarget],
    ) -> Result<(i64, i64, i64, i64), StorageError> {
        let owner_username = owner_username.trim();
        let requested = targets.len() as i64;
        let before_total = self.count_comment_targets(owner_username, "").await?;
        let mut added = 0_i64;

        for target in targets {
            if target.item_id.trim().is_empty() {
                continue;
            }

            let now = unix_seconds_now();
            let selected_at = if target.selected_at > 0.0 {
                target.selected_at
            } else {
                now
            };

            sqlx::query(
                r#"
                INSERT INTO comment_targets (
                    owner_username, item_id, title, price, pic_url, item_url, seller_id, seller_nick,
                    source, status, selected_at, created_at, updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(owner_username, item_id) DO UPDATE SET
                    title = excluded.title,
                    price = excluded.price,
                    pic_url = excluded.pic_url,
                    item_url = excluded.item_url,
                    seller_id = excluded.seller_id,
                    seller_nick = excluded.seller_nick,
                    source = excluded.source,
                    status = excluded.status,
                    selected_at = excluded.selected_at,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(owner_username)
            .bind(&target.item_id)
            .bind(&target.title)
            .bind(&target.price)
            .bind(&target.pic_url)
            .bind(&target.item_url)
            .bind(&target.seller_id)
            .bind(&target.seller_nick)
            .bind(if target.source.trim().is_empty() {
                "search"
            } else {
                target.source.as_str()
            })
            .bind(if target.status.trim().is_empty() {
                "pending"
            } else {
                target.status.as_str()
            })
            .bind(selected_at)
            .bind(now)
            .bind(now)
            .execute(&self.xianyu)
            .await?;

            added += 1;
        }

        let after_total = self.count_comment_targets(owner_username, "").await?;
        let deduplicated = (requested - (after_total - before_total).max(0)).max(0);
        Ok((requested, added, deduplicated, after_total))
    }

    pub async fn list_comment_targets(
        &self,
        owner_username: &str,
        status: &str,
        page: i64,
        page_size: i64,
    ) -> Result<Vec<CommentTarget>, StorageError> {
        let owner_username = owner_username.trim();
        let safe_page = page.max(1);
        let safe_page_size = page_size.max(1);
        let offset = (safe_page - 1) * safe_page_size;

        let rows = if status.trim().is_empty() {
            sqlx::query(
                r#"
                SELECT owner_username, item_id, title, price, pic_url, item_url, seller_id, seller_nick, source, status, selected_at, created_at, updated_at
                FROM comment_targets
                WHERE owner_username = ?
                ORDER BY selected_at DESC, updated_at DESC, item_id DESC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(owner_username)
            .bind(safe_page_size)
            .bind(offset)
            .fetch_all(&self.xianyu)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT owner_username, item_id, title, price, pic_url, item_url, seller_id, seller_nick, source, status, selected_at, created_at, updated_at
                FROM comment_targets
                WHERE owner_username = ? AND status = ?
                ORDER BY selected_at DESC, updated_at DESC, item_id DESC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(owner_username)
            .bind(status)
            .bind(safe_page_size)
            .bind(offset)
            .fetch_all(&self.xianyu)
            .await?
        };

        Ok(rows.into_iter().map(comment_target_from_row).collect())
    }

    pub async fn count_comment_targets(
        &self,
        owner_username: &str,
        status: &str,
    ) -> Result<i64, StorageError> {
        let owner_username = owner_username.trim();
        let total = if status.trim().is_empty() {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM comment_targets WHERE owner_username = ?",
            )
            .bind(owner_username)
            .fetch_one(&self.xianyu)
            .await?
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM comment_targets WHERE owner_username = ? AND status = ?",
            )
            .bind(owner_username)
            .bind(status)
            .fetch_one(&self.xianyu)
            .await?
        };

        Ok(total)
    }

    pub async fn list_comment_candidates(
        &self,
        uid: &str,
        quality: &str,
        category: &str,
        accepted_for_txt: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<(Vec<CommentCandidate>, i64, CommentCandidateSummary), StorageError> {
        let safe_page = page.max(1);
        let safe_page_size = page_size.max(1);
        let offset = (safe_page - 1) * safe_page_size;

        let mut where_clauses = vec!["uid = ?".to_string()];
        if !quality.trim().is_empty() {
            where_clauses.push("quality_label = ?".to_string());
        }
        if !category.trim().is_empty() {
            where_clauses.push("category = ?".to_string());
        }
        if accepted_for_txt.is_some() {
            where_clauses.push("accepted_for_txt = ?".to_string());
        }

        let where_sql = where_clauses.join(" AND ");
        let args = build_candidate_args(uid, quality, category, accepted_for_txt);
        let rows = execute_candidate_query(&self.xianyu, &where_sql, &args, safe_page_size, offset)
            .await?;
        let total = execute_candidate_count(&self.xianyu, &where_sql, &args).await?;
        let quality_summary = fetch_group_counts(
            &self.xianyu,
            "SELECT quality_label AS label, COUNT(*) AS count FROM item_comment_snapshots WHERE uid = ? GROUP BY quality_label",
            uid,
            &["bad", "good", "review"],
        )
        .await?;
        let category_summary = fetch_group_counts(
            &self.xianyu,
            "SELECT category AS label, COUNT(*) AS count FROM item_comment_snapshots WHERE uid = ? GROUP BY category",
            uid,
            &["其他", "价格", "包装", "商品", "服务", "物流"],
        )
        .await?;
        let accepted_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM item_comment_snapshots WHERE uid = ? AND accepted_for_txt = 1",
        )
        .bind(uid)
        .fetch_one(&self.xianyu)
        .await?;

        Ok((
            rows.into_iter().map(comment_candidate_from_row).collect(),
            total,
            CommentCandidateSummary {
                quality: counts_to_value_map(quality_summary),
                category: counts_to_value_map(category_summary),
                accepted_for_txt: accepted_count,
                total: self.count_comment_candidates(uid, "", "", None).await?,
            },
        ))
    }

    pub async fn count_comment_candidates(
        &self,
        uid: &str,
        quality: &str,
        category: &str,
        accepted_for_txt: Option<bool>,
    ) -> Result<i64, StorageError> {
        let args = build_candidate_args(uid, quality, category, accepted_for_txt);
        let mut where_clauses = vec!["uid = ?".to_string()];
        if !quality.trim().is_empty() {
            where_clauses.push("quality_label = ?".to_string());
        }
        if !category.trim().is_empty() {
            where_clauses.push("category = ?".to_string());
        }
        if accepted_for_txt.is_some() {
            where_clauses.push("accepted_for_txt = ?".to_string());
        }

        execute_candidate_count(&self.xianyu, &where_clauses.join(" AND "), &args).await
    }

    fn default_comment_candidates_dir(&self) -> PathBuf {
        self.databases
            .iter()
            .find(|database| database.name == "xianyu")
            .and_then(|database| Path::new(&database.path).parent().map(Path::to_path_buf))
            .map(|path| path.join("comment_candidates"))
            .unwrap_or_else(|| PathBuf::from("comment_candidates"))
    }

    pub async fn export_comment_candidates(
        &self,
        uid: &str,
        quality: &str,
        category: &str,
        accepted_for_txt: Option<bool>,
        directory: &str,
        file_name: &str,
    ) -> Result<CommentExportSummary, StorageError> {
        let (items, _, _) = self
            .list_comment_candidates(uid, quality, category, accepted_for_txt, 1, 100000)
            .await?;

        let target_dir = if directory.trim().is_empty() {
            self.default_comment_candidates_dir()
        } else {
            PathBuf::from(directory)
        };
        fs::create_dir_all(&target_dir)?;

        let output_name = if file_name.trim().is_empty() {
            format!("{}-{}.txt", uid, unix_millis_now())
        } else {
            file_name.to_string()
        };
        let file_path = target_dir.join(output_name);
        let tmp_path = file_path.with_extension("tmp");

        let mut content = String::new();
        let ids: Vec<i64> = items.iter().map(|item| item.id).collect();
        for candidate in &items {
            content.push_str(&candidate.content);
            content.push('\n');
        }
        fs::write(&tmp_path, content)?;
        fs::rename(&tmp_path, &file_path)?;

        if !ids.is_empty() {
            let mut builder = QueryBuilder::<Sqlite>::new(
                "UPDATE item_comment_snapshots SET accepted_for_txt = 1, txt_exported_at = ",
            );
            let exported_at = unix_seconds_now();
            builder
                .push_bind(exported_at)
                .push(", txt_export_path = ")
                .push_bind(file_path.display().to_string())
                .push(", updated_at = ")
                .push_bind(exported_at)
                .push(" WHERE id IN (");
            let mut separated = builder.separated(", ");
            for id in &ids {
                separated.push_bind(*id);
            }
            drop(separated);
            builder.push(")");
            builder.build().execute(&self.xianyu).await?;
        }

        Ok(CommentExportSummary {
            exported: items.len() as i64,
            duplicates: 0,
            skipped: 0,
            file_path: file_path.display().to_string(),
            error: String::new(),
        })
    }

    pub async fn import_comment_candidates(
        &self,
        uid: &str,
        file_path: &str,
        category: &str,
        source_api: &str,
        source_type: &str,
        item_id: &str,
    ) -> Result<CommentImportSummary, StorageError> {
        let raw = fs::read_to_string(file_path)?;
        let import_at = unix_seconds_now();
        let mut imported = 0_i64;
        let mut duplicates = 0_i64;
        let mut skipped = 0_i64;
        let normalized_category = if category.trim().is_empty() {
            "其他".to_string()
        } else {
            category.trim().to_string()
        };
        let mut seen = std::collections::HashSet::new();

        for line in raw.lines() {
            let content = normalize_comment_content(line);
            if content.is_empty() {
                skipped += 1;
                continue;
            }

            let content_hash = hash_comment_content(&content);
            if !seen.insert(content_hash.clone()) {
                duplicates += 1;
                continue;
            }

            let existing = sqlx::query("SELECT id FROM item_comment_snapshots WHERE uid = ? AND content_hash = ? ORDER BY id DESC LIMIT 1")
                .bind(uid)
                .bind(&content_hash)
                .fetch_optional(&self.xianyu)
                .await?;

            if let Some(existing) = existing {
                sqlx::query(
                    r#"
                    UPDATE item_comment_snapshots
                    SET accepted_for_txt = 1, category = ?, txt_imported_at = ?, updated_at = ?
                    WHERE id = ?
                    "#,
                )
                .bind(&normalized_category)
                .bind(import_at)
                .bind(import_at)
                .bind(existing.get::<i64, _>("id"))
                .execute(&self.xianyu)
                .await?;
                duplicates += 1;
                continue;
            }

            let (score, quality, breakdown) = score_comment_content(&content);
            sqlx::query(
                r#"
                INSERT INTO item_comment_snapshots (
                    uid, item_id, source_api, source_type, source_comment_key,
                    comment_id, author_id, author_nick, content, content_hash,
                    published_at, category, quality_score, quality_label, score_breakdown_json,
                    raw_json, snapshot_time, accepted_for_txt, txt_exported_at, txt_export_path,
                    txt_imported_at, created_at, updated_at
                )
                VALUES (?, ?, ?, ?, ?, '', '', '', ?, ?, '', ?, ?, ?, ?, ?, ?, 1, 0, '', ?, ?, ?)
                "#,
            )
            .bind(uid)
            .bind(if item_id.trim().is_empty() {
                "txt-import"
            } else {
                item_id
            })
            .bind(if source_api.trim().is_empty() {
                "txt_import"
            } else {
                source_api
            })
            .bind(if source_type.trim().is_empty() {
                "txt"
            } else {
                source_type
            })
            .bind(format!("txt:{content_hash}"))
            .bind(&content)
            .bind(&content_hash)
            .bind(&normalized_category)
            .bind(score)
            .bind(&quality)
            .bind(
                serde_json::to_string(&breakdown)
                    .map_err(|error| StorageError::Io(std::io::Error::other(error)))?,
            )
            .bind(
                serde_json::to_string(&serde_json::json!({
                    "imported_from_txt": true,
                    "file_path": file_path,
                    "category": normalized_category,
                }))
                .map_err(|error| StorageError::Io(std::io::Error::other(error)))?,
            )
            .bind(import_at)
            .bind(import_at)
            .bind(import_at)
            .bind(import_at)
            .execute(&self.xianyu)
            .await?;

            imported += 1;
        }

        Ok(CommentImportSummary {
            imported,
            duplicates,
            skipped,
            file_path: file_path.to_string(),
            error: String::new(),
        })
    }

    pub async fn list_comment_library(
        &self,
        owner_username: &str,
        enabled: Option<bool>,
    ) -> Result<Vec<CommentLibraryItem>, StorageError> {
        let owner_username = owner_username.trim();
        let rows = if let Some(enabled) = enabled {
            sqlx::query(
                r#"
                SELECT id, owner_username, content, content_hash, source, source_type, source_label, source_ref, source_item_id, enabled, usage_count, success_count, last_used_at, created_at, updated_at
                FROM comment_library
                WHERE owner_username = ? AND enabled = ?
                ORDER BY enabled DESC, updated_at DESC, id DESC
                "#,
            )
            .bind(owner_username)
            .bind(if enabled { 1_i64 } else { 0_i64 })
            .fetch_all(&self.xianyu)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, owner_username, content, content_hash, source, source_type, source_label, source_ref, source_item_id, enabled, usage_count, success_count, last_used_at, created_at, updated_at
                FROM comment_library
                WHERE owner_username = ?
                ORDER BY enabled DESC, updated_at DESC, id DESC
                "#,
            )
            .bind(owner_username)
            .fetch_all(&self.xianyu)
            .await?
        };

        Ok(rows.into_iter().map(comment_library_from_row).collect())
    }

    pub async fn count_comment_library(
        &self,
        owner_username: &str,
        enabled: Option<bool>,
    ) -> Result<i64, StorageError> {
        let owner_username = owner_username.trim();
        let total = if let Some(enabled) = enabled {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM comment_library WHERE owner_username = ? AND enabled = ?",
            )
            .bind(owner_username)
            .bind(if enabled { 1_i64 } else { 0_i64 })
            .fetch_one(&self.xianyu)
            .await?
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM comment_library WHERE owner_username = ?",
            )
            .bind(owner_username)
            .fetch_one(&self.xianyu)
            .await?
        };
        Ok(total)
    }

    pub async fn create_comment_library(
        &self,
        owner_username: &str,
        content: &str,
        source: &str,
        source_type: &str,
        source_label: &str,
        source_ref: &str,
        source_item_id: &str,
    ) -> Result<(CommentLibraryItem, bool), StorageError> {
        let normalized = normalize_comment_content(content);
        if normalized.is_empty() {
            return Err(StorageError::Io(std::io::Error::other(
                "content cannot be empty",
            )));
        }

        let owner_username = owner_username.trim();
        let content_hash = hash_comment_content(&normalized);
        if let Some(existing) = self
            .get_comment_library_item(owner_username, &content_hash)
            .await?
        {
            return Ok((existing, false));
        }

        let now = unix_seconds_now();
        sqlx::query(
            r#"
            INSERT INTO comment_library (
                owner_username, content, content_hash, source, source_type, source_label,
                source_ref, source_item_id, enabled, usage_count, success_count,
                last_used_at, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, 0, 0, 0, ?, ?)
            "#,
        )
        .bind(owner_username)
        .bind(&normalized)
        .bind(&content_hash)
        .bind(source)
        .bind(source_type)
        .bind(source_label)
        .bind(source_ref)
        .bind(source_item_id)
        .bind(now)
        .bind(now)
        .execute(&self.xianyu)
        .await?;

        Ok((
            self.get_comment_library_item(owner_username, &content_hash)
                .await?
                .ok_or_else(|| {
                    StorageError::Io(std::io::Error::other("created comment library row missing"))
                })?,
            true,
        ))
    }

    pub async fn batch_create_comment_library(
        &self,
        owner_username: &str,
        contents: &[String],
    ) -> Result<(i64, i64, i64), StorageError> {
        let mut imported = 0_i64;
        let mut duplicates = 0_i64;
        let mut skipped = 0_i64;

        for content in contents {
            let normalized = normalize_comment_content(content);
            if normalized.is_empty() {
                skipped += 1;
                continue;
            }
            let (_, created) = self
                .create_comment_library(
                    owner_username,
                    &normalized,
                    "comment_list",
                    "imported",
                    "评论列表导入",
                    "",
                    "",
                )
                .await?;
            if created {
                imported += 1;
            } else {
                duplicates += 1;
            }
        }

        Ok((imported, duplicates, skipped))
    }

    pub async fn import_comment_library_from_txt(
        &self,
        owner_username: &str,
        file_path: &str,
        source_label: &str,
        source_ref: &str,
        source_item_id: &str,
    ) -> Result<CommentImportSummary, StorageError> {
        let raw = fs::read_to_string(file_path)?;
        let mut imported = 0_i64;
        let mut duplicates = 0_i64;
        let mut skipped = 0_i64;
        let mut seen = std::collections::HashSet::new();

        for line in raw.lines() {
            let normalized = normalize_comment_content(line);
            if normalized.is_empty() {
                skipped += 1;
                continue;
            }
            let content_hash = hash_comment_content(&normalized);
            if !seen.insert(content_hash) {
                duplicates += 1;
                continue;
            }
            let (_, created) = self
                .create_comment_library(
                    owner_username,
                    &normalized,
                    "txt",
                    "txt",
                    if source_label.trim().is_empty() {
                        "txt_import"
                    } else {
                        source_label
                    },
                    if source_ref.trim().is_empty() {
                        file_path
                    } else {
                        source_ref
                    },
                    source_item_id,
                )
                .await?;
            if created {
                imported += 1;
            } else {
                duplicates += 1;
            }
        }

        Ok(CommentImportSummary {
            imported,
            duplicates,
            skipped,
            file_path: file_path.to_string(),
            error: String::new(),
        })
    }

    pub async fn import_comment_library_from_candidates(
        &self,
        owner_username: &str,
        uid: &str,
        quality: &str,
        category: &str,
    ) -> Result<(i64, i64, i64), StorageError> {
        let rows = execute_candidate_import_source(&self.xianyu, uid, quality, category).await?;
        let mut imported = 0_i64;
        let mut duplicates = 0_i64;
        let mut skipped = 0_i64;
        let mut seen = std::collections::HashSet::new();

        for row in rows {
            let content = row.get::<String, _>("content");
            let normalized = normalize_comment_content(&content);
            if normalized.is_empty() {
                skipped += 1;
                continue;
            }
            let content_hash = row.get::<String, _>("content_hash");
            if !seen.insert(content_hash.clone()) {
                duplicates += 1;
                continue;
            }

            let (_, created) = self
                .create_comment_library(
                    owner_username,
                    &normalized,
                    "candidate",
                    &row.get::<String, _>("source_type"),
                    "comment_candidates",
                    &row.get::<i64, _>("id").to_string(),
                    &row.get::<String, _>("item_id"),
                )
                .await?;
            if created {
                imported += 1;
            } else {
                duplicates += 1;
            }
        }

        Ok((imported, duplicates, skipped))
    }

    pub async fn set_comment_library_enabled(
        &self,
        owner_username: &str,
        content_hash: &str,
        enabled: bool,
    ) -> Result<Option<CommentLibraryItem>, StorageError> {
        let owner_username = owner_username.trim();
        let updated_at = unix_seconds_now();
        let result = sqlx::query(
            "UPDATE comment_library SET enabled = ?, updated_at = ? WHERE owner_username = ? AND content_hash = ?",
        )
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(updated_at)
        .bind(owner_username)
        .bind(content_hash)
        .execute(&self.xianyu)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.get_comment_library_item(owner_username, content_hash)
            .await
    }

    async fn get_comment_library_item(
        &self,
        owner_username: &str,
        content_hash: &str,
    ) -> Result<Option<CommentLibraryItem>, StorageError> {
        let owner_username = owner_username.trim();
        let row = sqlx::query(
            r#"
            SELECT id, owner_username, content, content_hash, source, source_type, source_label, source_ref, source_item_id, enabled, usage_count, success_count, last_used_at, created_at, updated_at
            FROM comment_library
            WHERE owner_username = ? AND content_hash = ?
            LIMIT 1
            "#,
        )
        .bind(owner_username)
        .bind(content_hash)
        .fetch_optional(&self.xianyu)
        .await?;

        Ok(row.map(comment_library_from_row))
    }
}

struct CandidateFilterArgs {
    uid: String,
    quality: Option<String>,
    category: Option<String>,
    accepted_for_txt: Option<bool>,
}

const COMMENT_QUALITY_DEFAULT: &str = "bad";
const COMMENT_CATEGORY_DEFAULT: &str = "其他";
const COMMENT_QUALITY_VALUES: [&str; 3] = ["bad", "good", "review"];
const COMMENT_CATEGORY_VALUES: [&str; 6] = ["其他", "价格", "包装", "商品", "服务", "物流"];
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

fn build_candidate_args(
    uid: &str,
    quality: &str,
    category: &str,
    accepted_for_txt: Option<bool>,
) -> CandidateFilterArgs {
    CandidateFilterArgs {
        uid: uid.trim().to_string(),
        quality: normalize_candidate_quality_input(quality),
        category: if category.trim().is_empty() {
            None
        } else {
            Some(normalize_comment_category(category))
        },
        accepted_for_txt,
    }
}

async fn execute_candidate_query(
    pool: &Pool<Sqlite>,
    where_sql: &str,
    args: &CandidateFilterArgs,
    limit: i64,
    offset: i64,
) -> Result<Vec<SqliteRow>, StorageError> {
    let sql = format!(
        "SELECT id, uid, item_id, source_api, source_type, comment_id, author_id, author_nick, content, content_hash, published_at, category, quality_score, quality_label, score_breakdown_json, snapshot_time, accepted_for_txt, txt_exported_at, txt_export_path, txt_imported_at FROM item_comment_snapshots WHERE {where_sql} ORDER BY quality_score DESC, snapshot_time DESC, id DESC LIMIT ? OFFSET ?"
    );
    let mut query = sqlx::query(&sql).bind(&args.uid);
    if let Some(quality) = &args.quality {
        query = query.bind(quality);
    }
    if let Some(category) = &args.category {
        query = query.bind(category);
    }
    if let Some(accepted_for_txt) = args.accepted_for_txt {
        query = query.bind(if accepted_for_txt { 1_i64 } else { 0_i64 });
    }
    query
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(StorageError::from)
}

async fn execute_candidate_count(
    pool: &Pool<Sqlite>,
    where_sql: &str,
    args: &CandidateFilterArgs,
) -> Result<i64, StorageError> {
    let sql = format!("SELECT COUNT(*) AS total FROM item_comment_snapshots WHERE {where_sql}");
    let mut query = sqlx::query_scalar::<_, i64>(&sql).bind(&args.uid);
    if let Some(quality) = &args.quality {
        query = query.bind(quality);
    }
    if let Some(category) = &args.category {
        query = query.bind(category);
    }
    if let Some(accepted_for_txt) = args.accepted_for_txt {
        query = query.bind(if accepted_for_txt { 1_i64 } else { 0_i64 });
    }
    query.fetch_one(pool).await.map_err(StorageError::from)
}

async fn fetch_group_counts(
    pool: &Pool<Sqlite>,
    sql: &str,
    uid: &str,
    defaults: &[&str],
) -> Result<Vec<(String, i64)>, StorageError> {
    let mut summary = defaults
        .iter()
        .map(|label| ((*label).to_string(), 0_i64))
        .collect::<Vec<_>>();
    let rows = sqlx::query(sql).bind(uid).fetch_all(pool).await?;

    for row in rows {
        let raw_label = row.get::<String, _>("label");
        let label = if defaults == COMMENT_CATEGORY_VALUES {
            normalize_comment_category(&raw_label)
        } else {
            normalize_comment_quality(&raw_label)
        };

        if let Some((_, count)) = summary.iter_mut().find(|(key, _)| key == &label) {
            *count = row.get::<i64, _>("count");
        } else {
            summary.push((label, row.get::<i64, _>("count")));
        }
    }

    summary.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(summary)
}

fn counts_to_value_map(counts: Vec<(String, i64)>) -> Map<String, Value> {
    let mut map = Map::new();
    for (label, count) in counts {
        map.insert(label, Value::from(count));
    }
    map
}

async fn execute_candidate_import_source(
    pool: &Pool<Sqlite>,
    uid: &str,
    quality: &str,
    category: &str,
) -> Result<Vec<SqliteRow>, StorageError> {
    let args = build_candidate_args(uid, quality, category, None);
    let mut where_clauses = vec!["uid = ?".to_string()];
    if args.quality.is_some() {
        where_clauses.push("quality_label = ?".to_string());
    }
    if args.category.is_some() {
        where_clauses.push("category = ?".to_string());
    }
    let sql = format!(
        "SELECT id, item_id, content, content_hash, category, source_type FROM item_comment_snapshots WHERE {} ORDER BY quality_score DESC, snapshot_time DESC, id DESC",
        where_clauses.join(" AND ")
    );

    let mut query = sqlx::query(&sql).bind(&args.uid);
    if let Some(quality) = &args.quality {
        query = query.bind(quality);
    }
    if let Some(category) = &args.category {
        query = query.bind(category);
    }

    query.fetch_all(pool).await.map_err(StorageError::from)
}

fn comment_target_from_row(row: SqliteRow) -> CommentTarget {
    CommentTarget {
        owner_username: row.get::<String, _>("owner_username"),
        item_id: row.get::<String, _>("item_id"),
        title: row.get::<String, _>("title"),
        price: row.get::<String, _>("price"),
        pic_url: row.get::<String, _>("pic_url"),
        item_url: row.get::<String, _>("item_url"),
        seller_id: row.get::<String, _>("seller_id"),
        seller_nick: row.get::<String, _>("seller_nick"),
        source: row.get::<String, _>("source"),
        status: row.get::<String, _>("status"),
        selected_at: row.get::<f64, _>("selected_at"),
        created_at: row.get::<f64, _>("created_at"),
        updated_at: row.get::<f64, _>("updated_at"),
    }
}

fn comment_candidate_from_row(row: SqliteRow) -> CommentCandidate {
    CommentCandidate {
        id: row.get::<i64, _>("id"),
        uid: row.get::<String, _>("uid"),
        item_id: row.get::<String, _>("item_id"),
        source_api: row.get::<String, _>("source_api"),
        source_type: row.get::<String, _>("source_type"),
        comment_id: row.get::<String, _>("comment_id"),
        author_id: row.get::<String, _>("author_id"),
        author_nick: row.get::<String, _>("author_nick"),
        content: row.get::<String, _>("content"),
        content_hash: row.get::<String, _>("content_hash"),
        published_at: row.get::<String, _>("published_at"),
        category: normalize_comment_category(&row.get::<String, _>("category")),
        score: row.get::<f64, _>("quality_score"),
        quality: normalize_comment_quality(&row.get::<String, _>("quality_label")),
        score_breakdown: parse_json_map(&row.get::<String, _>("score_breakdown_json")),
        snapshot_time: row.get::<f64, _>("snapshot_time"),
        accepted_for_txt: row.get::<i64, _>("accepted_for_txt") != 0,
        txt_exported_at: row.get::<f64, _>("txt_exported_at"),
        txt_export_path: row.get::<String, _>("txt_export_path"),
        txt_imported_at: row.get::<f64, _>("txt_imported_at"),
    }
}

fn comment_library_from_row(row: SqliteRow) -> CommentLibraryItem {
    CommentLibraryItem {
        id: row.get::<i64, _>("id"),
        owner_username: row.get::<String, _>("owner_username"),
        content: row.get::<String, _>("content"),
        content_hash: row.get::<String, _>("content_hash"),
        source: row.get::<String, _>("source"),
        source_type: row.get::<String, _>("source_type"),
        source_label: row.get::<String, _>("source_label"),
        source_ref: row.get::<String, _>("source_ref"),
        source_item_id: row.get::<String, _>("source_item_id"),
        enabled: row.get::<i64, _>("enabled") != 0,
        usage_count: row.get::<i64, _>("usage_count"),
        success_count: row.get::<i64, _>("success_count"),
        last_used_at: row.get::<f64, _>("last_used_at"),
        created_at: row.get::<f64, _>("created_at"),
        updated_at: row.get::<f64, _>("updated_at"),
    }
}

fn normalize_comment_content(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hash_comment_content(content: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn score_comment_content(content: &str) -> (f64, String, Map<String, Value>) {
    let text = normalize_comment_content(content);
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
    let lowered = text.to_lowercase();
    if spam_markers.iter().any(|marker| lowered.contains(marker)) {
        penalties += 0.45;
    }
    if has_repeated_run(&text, 4) {
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

fn normalize_comment_quality(raw: &str) -> String {
    let normalized = normalize_comment_content(raw).to_lowercase();
    if COMMENT_QUALITY_VALUES.contains(&normalized.as_str()) {
        normalized
    } else {
        COMMENT_QUALITY_DEFAULT.to_string()
    }
}

fn normalize_candidate_quality_input(raw: &str) -> Option<String> {
    let normalized = normalize_comment_content(raw).to_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalize_comment_quality(&normalized))
    }
}

fn normalize_comment_category(raw: &str) -> String {
    let normalized = normalize_comment_content(raw);
    if normalized.is_empty() {
        return COMMENT_CATEGORY_DEFAULT.to_string();
    }
    for (category, keywords) in COMMENT_CATEGORY_RULES {
        if normalized == category || keywords.iter().any(|keyword| normalized.contains(keyword)) {
            return category.to_string();
        }
    }
    COMMENT_CATEGORY_DEFAULT.to_string()
}

fn has_repeated_run(text: &str, min_run: usize) -> bool {
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
