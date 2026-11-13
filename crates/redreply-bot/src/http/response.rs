use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_TYPE, LOCATION, SET_COOKIE};
use hyper::{Response, StatusCode};
use serde::Serialize;

/// 框架内 HTTP body 的错误类型。
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// 框架统一响应 body。
pub type HttpBody = BoxBody<Bytes, BoxError>;

/// 框架统一响应。
pub type HttpResponse = Response<HttpBody>;

/// 路由处理函数返回的 future。
pub type RouteFuture =
    Pin<Box<dyn Future<Output = std::result::Result<HttpResponse, HttpError>> + Send + 'static>>;

/// 路由处理错误；会被转换为带 `retcode` 的 JSON 响应。
#[derive(Debug)]
pub struct HttpError {
    pub status: StatusCode,
    pub message: String,
}

impl HttpError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "api not found")
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn into_response(self) -> HttpResponse {
        json_response(
            self.status,
            &serde_json::json!({"retcode": -1, "data": {"message": self.message}}),
        )
    }
}

impl From<crate::FrameworkError> for HttpError {
    fn from(error: crate::FrameworkError) -> Self {
        match error {
            crate::FrameworkError::Service(message) => Self::conflict(message),
            crate::FrameworkError::Plugin(message) => Self::new(StatusCode::CONFLICT, message),
            other => Self::internal(other.to_string()),
        }
    }
}

/// 把字节包装成响应 body。
pub fn full(bytes: impl Into<Bytes>) -> HttpBody {
    BoxBody::new(Full::new(bytes.into()).map_err(|never| match never {}))
}

/// 空 body。
pub fn empty() -> HttpBody {
    BoxBody::new(Empty::<Bytes>::new().map_err(|never| match never {}))
}

pub fn with_body(status: StatusCode, body: HttpBody) -> HttpResponse {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    response
}

/// JSON 响应。
pub fn json_response<T: Serialize>(status: StatusCode, value: &T) -> HttpResponse {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = with_body(status, full(bytes));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

/// 文本响应。
pub fn text_response(status: StatusCode, text: impl Into<String>) -> HttpResponse {
    let mut response = with_body(status, full(text.into()));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// 指定内容类型的字节响应。
pub fn bytes_response(
    status: StatusCode,
    content_type: &str,
    bytes: impl AsRef<[u8]>,
    cache_control: Option<&str>,
) -> HttpResponse {
    let mut response = with_body(status, full(Bytes::copy_from_slice(bytes.as_ref())));
    if let Ok(value) = HeaderValue::from_str(content_type) {
        response.headers_mut().insert(CONTENT_TYPE, value);
    }
    if let Some(cache) = cache_control {
        if let Ok(value) = HeaderValue::from_str(cache) {
            response.headers_mut().insert(CACHE_CONTROL, value);
        }
    }
    response
}

/// 302 重定向。
pub fn redirect_response(location: &str) -> HttpResponse {
    let mut response = with_body(StatusCode::FOUND, empty());
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(LOCATION, value);
    }
    response
}

/// 302 重定向并设置 Cookie。
pub fn redirect_with_cookie(location: &str, cookie: &str) -> HttpResponse {
    let mut response = redirect_response(location);
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    response
}

/// 400 业务错误响应（保持与旧版一致的 `retcode:-1` 结构）。
pub fn error_response(status: StatusCode, message: impl Into<String>) -> HttpResponse {
    HttpError::new(status, message).into_response()
}
