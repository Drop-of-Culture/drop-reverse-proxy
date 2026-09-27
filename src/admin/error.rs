use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

#[derive(Debug)]
pub enum AdminError {
    /// No identity forwarded by oauth2-proxy.
    Unauthorized,
    /// Identity forwarded, but not an active admin or not enough rights.
    Forbidden(String),
    NotFound,
    /// The change breaks a database constraint (still referenced, duplicate...).
    Conflict(String),
    Validation(String),
    Internal(String),
}

impl From<sqlx::Error> for AdminError {
    fn from(err: sqlx::Error) -> Self {
        match &err {
            sqlx::Error::RowNotFound => AdminError::NotFound,
            sqlx::Error::Database(db_err) => match db_err.code().as_deref() {
                // foreign_key_violation: on insert/update the referenced row is missing,
                // on delete the row is still referenced
                Some("23503") if db_err.message().starts_with("insert or update") => {
                    AdminError::Validation("A selected item no longer exists.".to_string())
                }
                Some("23503") => AdminError::Conflict(
                    "This item is still used by other items, remove or change them first.".to_string(),
                ),
                // unique_violation
                Some("23505") => AdminError::Conflict("This item already exists.".to_string()),
                _ => AdminError::Internal(err.to_string()),
            },
            _ => AdminError::Internal(err.to_string()),
        }
    }
}

#[derive(Template)]
#[template(path = "admin/error.html")]
struct ErrorTemplate<'a> {
    status: u16,
    message: &'a str,
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AdminError::Unauthorized => (StatusCode::UNAUTHORIZED, "Not authenticated."),
            AdminError::Forbidden(message) => (StatusCode::FORBIDDEN, message.as_str()),
            AdminError::NotFound => (StatusCode::NOT_FOUND, "Not found."),
            AdminError::Conflict(message) => (StatusCode::CONFLICT, message.as_str()),
            AdminError::Validation(message) => (StatusCode::UNPROCESSABLE_ENTITY, message.as_str()),
            AdminError::Internal(detail) => {
                tracing::error!(detail, "admin internal error");
                println!("admin internal error: {detail}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error.")
            }
        };
        let body = ErrorTemplate { status: status.as_u16(), message }
            .render()
            .unwrap_or_else(|_| message.to_string());
        (status, Html(body)).into_response()
    }
}
