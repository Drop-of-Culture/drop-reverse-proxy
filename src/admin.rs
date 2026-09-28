//! Back-office used to manage artists, artworks, drops, tags...
//!
//! It is meant to run behind oauth2-proxy: authentication (GitHub login) is done
//! by the proxy, which forwards the login in the `X-Forwarded-User` header.
//! This app only decides, from the `admin_user` table, whether that login is
//! allowed in and with which role. Because the header is trusted, the admin
//! app must only listen on a loopback address.

use askama::Template;
use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::response::Html;
use axum::routing::get;
use figment::Figment;
use figment::providers::{Format, Toml};
use serde::Deserialize;
use sqlx::PgPool;
use std::sync::Arc;

pub mod admin_user;
pub mod artist;
pub mod artwork;
pub mod audit;
pub mod audit_log;
pub mod auth;
pub mod drop;
pub mod drop_type;
pub mod error;
pub mod ip;
pub mod page;
pub mod playlist;
pub mod redirect;
pub mod tag;
pub mod token;

use auth::AdminUser;
use error::AdminError;

#[derive(Clone, Deserialize, Debug)]
pub struct AdminConf {
    bind_addr: String,
    /// Public origin of the back-office as seen by the browser (scheme://host[:port]),
    /// used to reject cross-site form posts.
    allowed_origin: String,
}

impl AdminConf {
    pub fn bind_addr(&self) -> &str {
        &self.bind_addr
    }

    pub fn allowed_origin(&self) -> &str {
        &self.allowed_origin
    }
}

/// Reads the `[admin_conf]` section of the given toml file.
pub fn create_admin_conf_from_toml_file(relative_path: &str) -> figment::Result<AdminConf> {
    Figment::new()
        .merge(Toml::file(relative_path))
        .focus("admin_conf")
        .extract()
}

#[derive(Clone)]
pub struct AdminState {
    pub pool: PgPool,
    pub allowed_origin: Arc<str>,
    /// Bad attempts from which the proxy refuses an IP (`max_attempts` in app.toml).
    pub max_attempts: u8,
}

pub fn admin_app(state: AdminState) -> Router {
    Router::new()
        .route("/", get(index))
        .merge(artist::routes())
        .merge(artwork::routes())
        .merge(drop::routes())
        .merge(drop_type::routes())
        .merge(tag::routes())
        .merge(redirect::routes())
        .merge(playlist::routes())
        .merge(token::routes())
        .merge(ip::routes())
        .merge(admin_user::routes())
        .merge(audit_log::routes())
        // layers run bottom-up: identify the admin first, then check the origin
        .layer(from_fn_with_state(state.clone(), auth::check_origin))
        .layer(from_fn_with_state(state.clone(), auth::require_admin))
        .layer(crate::http_trace_layer())
        .with_state(state)
}

#[derive(Template)]
#[template(path = "admin/index.html")]
struct IndexTemplate {
    admin: AdminUser,
}

async fn index(admin: AdminUser) -> Result<Html<String>, AdminError> {
    render(&IndexTemplate { admin })
}

pub(crate) fn render(template: &impl Template) -> Result<Html<String>, AdminError> {
    template
        .render()
        .map(Html)
        .map_err(|err| AdminError::Internal(err.to_string()))
}
