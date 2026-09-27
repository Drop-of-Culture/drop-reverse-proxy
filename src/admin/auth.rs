use crate::admin::AdminState;
use crate::admin::error::AdminError;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::{ORIGIN, REFERER};
use axum::http::request::Parts;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;

/// Header set by oauth2-proxy (`pass_user_headers = true`), holding the GitHub login.
pub const USER_HEADER: &str = "x-forwarded-user";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Owner,
    Editor,
}

impl TryFrom<&str> for Role {
    type Error = AdminError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "owner" => Ok(Role::Owner),
            "editor" => Ok(Role::Editor),
            other => Err(AdminError::Internal(format!("unknown admin role '{other}'"))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AdminUser {
    pub github_login: String,
    pub role: Role,
}

impl AdminUser {
    pub fn is_owner(&self) -> bool {
        self.role == Role::Owner
    }

    pub fn require_owner(&self) -> Result<(), AdminError> {
        if self.is_owner() {
            Ok(())
        } else {
            Err(AdminError::Forbidden("Only owners can do this.".to_string()))
        }
    }
}

/// Available in any handler behind `require_admin`.
impl<S: Send + Sync> FromRequestParts<S> for AdminUser {
    type Rejection = AdminError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AdminUser>()
            .cloned()
            .ok_or(AdminError::Unauthorized)
    }
}

/// Maps the login forwarded by oauth2-proxy to an active admin, or rejects the request.
pub async fn require_admin(
    State(state): State<AdminState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AdminError> {
    let login = req
        .headers()
        .get(USER_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|login| !login.is_empty())
        .ok_or(AdminError::Unauthorized)?
        .to_string();

    // GitHub logins are case-insensitive
    let (github_login, role) = sqlx::query_as::<_, (String, String)>("
SELECT github_login, role
FROM \"admin_user\"
WHERE lower(github_login) = lower($1) AND active
LIMIT 1
")
        .bind(&login)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AdminError::Forbidden(format!("'{login}' is not an admin.")))?;

    let role = Role::try_from(role.as_str())?;
    req.extensions_mut().insert(AdminUser { github_login, role });
    Ok(next.run(req).await)
}

/// Rejects state-changing requests that don't come from the back-office pages (CSRF).
pub async fn check_origin(
    State(state): State<AdminState>,
    req: Request,
    next: Next,
) -> Result<Response, AdminError> {
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        || is_same_origin(&req, &state.allowed_origin)
    {
        Ok(next.run(req).await)
    } else {
        Err(AdminError::Forbidden("Cross-origin request refused.".to_string()))
    }
}

fn is_same_origin(req: &Request, allowed_origin: &str) -> bool {
    let header = |name| req.headers().get(name).and_then(|value| value.to_str().ok());
    match (header(ORIGIN), header(REFERER)) {
        (Some(origin), _) => origin == allowed_origin,
        (None, Some(referer)) => {
            referer == allowed_origin
                || referer
                    .strip_prefix(allowed_origin)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
        (None, None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn post_with(header: Option<(&str, &str)>) -> Request {
        let mut builder = Request::builder().method(Method::POST).uri("/artists");
        if let Some((name, value)) = header {
            builder = builder.header(name, value);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn same_origin_is_accepted() {
        let origin = "https://admin.example.com";
        assert!(is_same_origin(&post_with(Some(("origin", origin))), origin));
        assert!(is_same_origin(&post_with(Some(("referer", "https://admin.example.com/artists"))), origin));
    }

    #[test]
    fn other_origins_are_refused() {
        let origin = "https://admin.example.com";
        assert!(!is_same_origin(&post_with(None), origin));
        assert!(!is_same_origin(&post_with(Some(("origin", "https://evil.example.com"))), origin));
        assert!(!is_same_origin(&post_with(Some(("origin", "https://admin.example.com.evil.com"))), origin));
        assert!(!is_same_origin(&post_with(Some(("referer", "https://admin.example.com.evil.com/x"))), origin));
    }
}
