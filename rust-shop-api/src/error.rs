//! 统一错误分层（17 课 §3）
//!
//! AppError 是业务的"错误语言"；IntoResponse 决定它说成什么 HTTP 状态码。
//! handler 里只管 `?` 传播，映射集中在这里。

use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("资源不存在")]
    NotFound,

    #[error("商品 {0} 库存不足")]
    OutOfStock(i64),

    #[error("重复创建：{0}")]
    Conflict(String),

    /// 数据库错误统一兜底；唯一约束冲突单独映射为 409
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::OutOfStock(_) => (StatusCode::CONFLICT, self.to_string()),
            AppError::Conflict(_) => (StatusCode::CONFLICT, self.to_string()),
            AppError::Db(e) => match e {
                sqlx::Error::RowNotFound => {
                    (StatusCode::NOT_FOUND, "资源不存在".to_string())
                }
                // SQLite UNIQUE 违反码 2067；PG 是 23505（17 课速查表）
                sqlx::Error::Database(db)
                    if db.code().as_deref() == Some("2067") =>
                {
                    (StatusCode::CONFLICT, "唯一约束冲突".to_string())
                }
                sqlx::Error::PoolTimedOut => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "数据库繁忙，请稍后重试".to_string(),
                ),
                _ => {
                    // 500 不能漏内部细节给客户端，但日志要留全
                    tracing::error!("db error: {e:?}");
                    (StatusCode::INTERNAL_SERVER_ERROR, "服务器内部错误".to_string())
                }
            },
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

/// handler 通用返回类型：成功直接返回 T（Json），失败走 AppError
pub type ApiResult<T> = Result<Json<T>, AppError>;
