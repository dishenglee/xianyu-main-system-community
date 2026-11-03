use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json as ResponseJson,
};
use serde_json::{json, Value};

pub(crate) type ApiResult = Result<ResponseJson<Value>, ResponseError>;

#[derive(Debug)]
pub(crate) struct ResponseError {
    status: StatusCode,
    detail: String,
    public_detail: Option<String>,
}

impl ResponseError {
    pub(crate) fn status(&self) -> StatusCode {
        self.status
    }

    pub(crate) fn detail(&self) -> &str {
        &self.detail
    }

    pub(crate) fn public_detail(&self) -> Option<&str> {
        self.public_detail.as_deref()
    }

    pub(crate) fn bad_request(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            status: StatusCode::BAD_REQUEST,
            public_detail: Some(detail.clone()),
            detail,
        }
    }

    pub(crate) fn not_found(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            status: StatusCode::NOT_FOUND,
            public_detail: Some(detail.clone()),
            detail,
        }
    }

    pub(crate) fn forbidden(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            status: StatusCode::FORBIDDEN,
            public_detail: Some(detail.clone()),
            detail,
        }
    }

    pub(crate) fn private(status: StatusCode, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
            public_detail: None,
        }
    }

    pub(crate) fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            detail: error.to_string(),
            public_detail: None,
        }
    }

    pub(crate) fn internal_message(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            public_detail: Some(detail.clone()),
            detail,
        }
    }

    pub(crate) fn service_unavailable(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            public_detail: Some(detail.clone()),
            detail,
        }
    }

    pub(crate) fn multipart(error: axum::extract::multipart::MultipartError) -> Self {
        Self::private(StatusCode::BAD_REQUEST, format!("multipart error: {error}"))
    }
}

impl IntoResponse for ResponseError {
    fn into_response(self) -> Response {
        (
            self.status,
            ResponseJson(json!({
                "detail": self.public_detail.unwrap_or_else(|| "服务暂时不可用，请稍后重试".to_string()),
                "status": "error",
            })),
        )
            .into_response()
    }
}

impl std::fmt::Display for ResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.detail.fmt(f)
    }
}

impl std::error::Error for ResponseError {}
