use axum::{
    extract::{Extension, Request, State},
    http::header::CONTENT_TYPE,
    Json as ResponseJson,
};
use multer::{Constraints, Multipart, SizeLimit};
use serde_json::{json, Value};
use tracing::warn;

use crate::{
    models::auth::RequestIdentity,
    routes::accounts::runtime_access::{
        ensure_account_can_call_upstream, load_account_owner_username,
    },
    routes::error::{ApiResult, ResponseError},
    state::AppState,
    xianyu_client,
};

use super::ensure_market_uid_access;

const UPLOAD_BODY_LIMIT_BYTES: u64 = 32 * 1024 * 1024;
const UID_FIELD_LIMIT_BYTES: u64 = 16 * 1024;

pub(super) async fn upload_item_image(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    request: Request,
) -> ApiResult {
    upload_common(state, &identity, request, false).await
}

pub(super) async fn upload_chat_image(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    request: Request,
) -> ApiResult {
    upload_common(state, &identity, request, true).await
}

async fn upload_common(
    state: AppState,
    identity: &RequestIdentity,
    request: Request,
    chat: bool,
) -> ApiResult {
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let boundary = multer::parse_boundary(&content_type)
        .map_err(|error| multipart_error(error.to_string()))?;
    let constraints = Constraints::new()
        .allowed_fields(vec!["uid", "file"])
        .size_limit(
            SizeLimit::new()
                .whole_stream(UPLOAD_BODY_LIMIT_BYTES)
                .per_field(UPLOAD_BODY_LIMIT_BYTES)
                .for_field("uid", UID_FIELD_LIMIT_BYTES),
        );
    let mut multipart = Multipart::with_constraints(
        request.into_body().into_data_stream(),
        boundary,
        constraints,
    );

    let mut uid = String::new();
    let mut filename = "image.jpg".to_string();
    let mut content_type = "image/jpeg".to_string();
    let mut size = 0_usize;
    let mut bytes = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| multipart_error(error.to_string()))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name == "uid" {
            uid = field
                .text()
                .await
                .map_err(|error| multipart_error(error.to_string()))?;
        } else if name == "file" {
            filename = field.file_name().unwrap_or("image.jpg").to_string();
            content_type = field
                .content_type()
                .map(|mime| mime.to_string())
                .unwrap_or_else(|| "image/jpeg".to_string());
            bytes = field
                .bytes()
                .await
                .map_err(|error| multipart_error(error.to_string()))?
                .to_vec();
            size = bytes.len();
        }
    }

    if !uid.trim().is_empty() {
        ensure_market_uid_access(&state, identity, &uid).await?;
        let owner_username = load_account_owner_username(&state, &uid).await?;
        ensure_account_can_call_upstream(&state, owner_username.as_deref(), &uid).await?;
        let upload_result = if chat {
            xianyu_client::stream_upload(
                state.config(),
                state.storage().accounts(),
                &uid,
                &filename,
                &content_type,
                bytes.clone(),
                true,
            )
            .await
        } else {
            match xianyu_client::stream_upload(
                state.config(),
                state.storage().accounts(),
                &uid,
                &filename,
                &content_type,
                bytes.clone(),
                false,
            )
            .await
            {
                Ok(result) => Ok(result),
                Err(stream_error) => {
                    warn!("stream upload failed for uid={uid}: {stream_error}");
                    match xianyu_client::taobao_upload(
                        state.config(),
                        state.storage().accounts(),
                        &uid,
                        &filename,
                        &content_type,
                        bytes.clone(),
                    )
                    .await
                    {
                        Ok(result) => Ok(result),
                        Err(taobao_error) => Err(format!(
                            "stream upload failed: {stream_error}; taobao upload failed: {taobao_error}"
                        )),
                    }
                }
            }
        };
        match upload_result {
            Ok(result) => {
                if chat {
                    return Ok(ResponseJson(json!({
                        "success": true,
                        "url": result.url,
                        "width": result.width,
                        "height": result.height,
                    })));
                }
                return Ok(ResponseJson(json!({
                    "success": true,
                    "url": result.url,
                })));
            }
            Err(error) => {
                warn!("upload failed for uid={uid}: {error}");
                return Err(ResponseError::bad_request(format!("图片上传失败：{error}")));
            }
        }
    }

    let _ = size;
    let _ = filename;
    Err(ResponseError::bad_request("缺少有效账号，无法上传图片"))
}

/// Upload an image from a remote URL — downloads it then re-uploads to Xianyu CDN.
/// Used by batch-list (一键上架) when import_link returns empty images.
pub(super) async fn upload_image_url(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::extract::Json(payload): axum::extract::Json<serde_json::Value>,
) -> ApiResult {
    let uid = payload
        .get("uid")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if uid.is_empty() || url.is_empty() {
        return Err(ResponseError::bad_request("缺少 uid 或 url"));
    }
    ensure_market_uid_access(&state, &identity, uid).await?;
    let owner_username = load_account_owner_username(&state, uid).await?;
    ensure_account_can_call_upstream(&state, owner_username.as_deref(), uid).await?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| ResponseError::internal_message(e.to_string()))?;

    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0")
        .send()
        .await
        .map_err(|e| ResponseError::bad_request(format!("下载图片失败：{e}")))?;

    let ct = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| ResponseError::bad_request(format!("读取图片失败：{e}")))?;
    if bytes.is_empty() {
        return Err(ResponseError::bad_request("图片内容为空"));
    }

    let ext = url
        .split('?')
        .next()
        .unwrap_or(url)
        .rsplit('.')
        .next()
        .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("jpg");
    let filename = format!("reupload.{ext}");

    let result = xianyu_client::stream_upload(
        state.config(),
        state.storage().accounts(),
        uid,
        &filename,
        &ct,
        bytes.to_vec(),
        false,
    )
    .await
    .map_err(|e| ResponseError::bad_request(format!("上传失败：{e}")))?;

    Ok(ResponseJson(json!({
        "success": true,
        "url": result.url,
    })))
}
fn multipart_error(detail: String) -> ResponseError {
    ResponseError::bad_request(format!("multipart error: {detail}"))
}
