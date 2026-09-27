//! Generic list and form pages shared by all entities, plus form helpers.

use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::render;
use askama::Template;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use sqlx::PgPool;

/// One option of a `<select>`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

pub struct Field {
    pub name: &'static str,
    pub label: &'static str,
    /// html input type: text, url, number
    pub input_type: &'static str,
    pub value: String,
    pub max_len: usize,
    pub required: bool,
    pub help: &'static str,
    /// Some: rendered as a `<select>`
    pub choices: Option<Vec<Choice>>,
}

impl Field {
    pub fn text(name: &'static str, label: &'static str, value: impl ToString, max_len: usize) -> Self {
        Self {
            name,
            label,
            input_type: "text",
            value: value.to_string(),
            max_len,
            required: true,
            help: "",
            choices: None,
        }
    }

    pub fn select(name: &'static str, label: &'static str, value: impl ToString, choices: Vec<Choice>) -> Self {
        Self { choices: Some(choices), ..Self::text(name, label, value, 0) }
    }

    pub fn input_type(mut self, input_type: &'static str) -> Self {
        self.input_type = input_type;
        self
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn help(mut self, help: &'static str) -> Self {
        self.help = help;
        self
    }
}

#[derive(Template)]
#[template(path = "admin/form.html")]
pub struct FormPage {
    pub admin: AdminUser,
    pub title: String,
    pub action: String,
    pub cancel_url: &'static str,
    pub fields: Vec<Field>,
    pub error: Option<String>,
}

impl FormPage {
    /// 200 for a blank form, 422 when re-displayed with a validation error.
    pub fn respond(self) -> Result<Response, AdminError> {
        let status = if self.error.is_some() { StatusCode::UNPROCESSABLE_ENTITY } else { StatusCode::OK };
        Ok((status, render(&self)?).into_response())
    }
}

/// A button posting to `url`, shown at the end of a list row.
pub struct RowAction {
    pub label: &'static str,
    pub url: String,
    pub danger: bool,
}

pub struct Row {
    pub cells: Vec<String>,
    pub edit_url: Option<String>,
    pub actions: Vec<RowAction>,
}

#[derive(Template)]
#[template(path = "admin/list.html")]
pub struct ListPage {
    pub admin: AdminUser,
    pub title: &'static str,
    pub note: Option<String>,
    pub new_url: Option<&'static str>,
    pub columns: Vec<&'static str>,
    pub rows: Vec<Row>,
}

impl ListPage {
    pub fn respond(self) -> Result<Response, AdminError> {
        Ok(render(&self)?.into_response())
    }
}

/// Deleting is reserved to owners: editors get no delete button.
pub fn delete_action(admin: &AdminUser, url: String) -> Vec<RowAction> {
    if admin.is_owner() {
        vec![RowAction { label: "Delete", url, danger: true }]
    } else {
        Vec::new()
    }
}

pub fn redirect_to(url: &str) -> Result<Response, AdminError> {
    Ok(Redirect::to(url).into_response())
}

/// Trimmed, non-empty, at most `max_len` characters.
pub fn required(value: &str, label: &str, max_len: usize) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{label} is required."))
    } else {
        optional(value, label, max_len)
    }
}

/// Trimmed, possibly empty, at most `max_len` characters.
pub fn optional(value: &str, label: &str, max_len: usize) -> Result<String, String> {
    let value = value.trim();
    if value.chars().count() > max_len {
        Err(format!("{label} must be at most {max_len} characters."))
    } else {
        Ok(value.to_string())
    }
}

pub async fn artist_choices(pool: &PgPool) -> Result<Vec<Choice>, AdminError> {
    choices(pool, "SELECT id::text AS value, name AS label FROM \"artist\" ORDER BY name").await
}

pub async fn artwork_choices(pool: &PgPool) -> Result<Vec<Choice>, AdminError> {
    choices(pool, "
SELECT aw.id::text AS value, aw.name || ' (' || a.name || ')' AS label
FROM \"artwork\" aw
JOIN \"artist\" a ON a.id = aw.artist_id
ORDER BY aw.name
").await
}

pub async fn drop_choices(pool: &PgPool) -> Result<Vec<Choice>, AdminError> {
    choices(pool, "SELECT id::text AS value, name || ' (#' || id || ')' AS label FROM \"drop\" ORDER BY name").await
}

pub async fn drop_type_choices(pool: &PgPool) -> Result<Vec<Choice>, AdminError> {
    choices(pool, "SELECT id::text AS value, name || ' (' || id || ')' AS label FROM \"drop_type\" ORDER BY id").await
}

async fn choices(pool: &PgPool, query: &'static str) -> Result<Vec<Choice>, AdminError> {
    Ok(sqlx::query_as::<_, Choice>(query).fetch_all(pool).await?)
}
