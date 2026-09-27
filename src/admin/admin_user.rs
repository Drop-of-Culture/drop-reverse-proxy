//! Back-office admins, managed by owners. Admins are disabled rather than deleted,
//! so the audit log keeps pointing to known logins.

use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{Choice, Field, FormPage, ListPage, Row, redirect_to, required};
use crate::admin::AdminState;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;

const ENTITY: &str = "admin_user";
const LIST_URL: &str = "/admins";
const ROLES: [&str; 2] = ["editor", "owner"];

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/admins", get(list).post(create))
        .route("/admins/new", get(new_form))
        .route("/admins/{login}/edit", get(edit_form))
        .route("/admins/{login}", post(update))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct AdminRecord {
    github_login: String,
    role: String,
    active: bool,
}

#[derive(Deserialize, Debug)]
struct AdminForm {
    #[serde(default)]
    github_login: String,
    role: String,
    #[serde(default = "default_active")]
    active: String,
}

fn default_active() -> String {
    "true".to_string()
}

impl Default for AdminForm {
    fn default() -> Self {
        Self { github_login: String::new(), role: "editor".to_string(), active: default_active() }
    }
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    admin.require_owner()?;
    let admins = sqlx::query_as::<_, (String, String, bool, String)>("
SELECT github_login, role, active, to_char(create_date, 'YYYY-MM-DD HH24:MI')
FROM \"admin_user\"
ORDER BY active DESC, github_login
")
        .fetch_all(&state.pool)
        .await?;

    let rows = admins.into_iter().map(|(login, role, active, created)| Row {
        edit_url: Some(format!("/admins/{login}/edit")),
        actions: Vec::new(),
        cells: vec![login, role, if active { "yes" } else { "no" }.to_string(), created],
    }).collect();

    ListPage {
        admin,
        title: "Admins",
        note: Some("Admins sign in with GitHub. Editors manage content, owners can also delete and manage admins.".to_string()),
        new_url: Some("/admins/new"),
        columns: vec!["GitHub login", "Role", "Active", "Added"],
        rows,
    }.respond()
}

fn form(admin: AdminUser, login: Option<&str>, form: AdminForm, error: Option<String>) -> Result<Response, AdminError> {
    let choices = |values: &[(&str, &str)]| {
        values.iter().map(|(value, label)| Choice { value: value.to_string(), label: label.to_string() }).collect()
    };
    let mut fields = Vec::new();
    if login.is_none() {
        fields.push(Field::text("github_login", "GitHub login", form.github_login, 39));
    }
    fields.push(Field::select("role", "Role", form.role, choices(&[("editor", "editor"), ("owner", "owner")])));
    if login.is_some() {
        fields.push(Field::select("active", "Active", form.active, choices(&[("true", "yes"), ("false", "no")])));
    }
    FormPage {
        admin,
        title: login.map_or("New admin".to_string(), |login| format!("Edit admin {login}")),
        action: login.map_or(LIST_URL.to_string(), |login| format!("/admins/{login}")),
        cancel_url: LIST_URL,
        fields,
        error,
    }.respond()
}

async fn new_form(admin: AdminUser) -> Result<Response, AdminError> {
    admin.require_owner()?;
    form(admin, None, AdminForm::default(), None)
}

async fn edit_form(admin: AdminUser, State(state): State<AdminState>, Path(login): Path<String>) -> Result<Response, AdminError> {
    admin.require_owner()?;
    let mut conn = state.pool.acquire().await?;
    let record = fetch(&mut conn, &login, false).await?;
    let input = AdminForm { github_login: record.github_login, role: record.role, active: record.active.to_string() };
    form(admin, Some(&login), input, None)
}

fn validate_login(value: &str) -> Result<String, String> {
    let login = required(value, "GitHub login", 39)?;
    let valid = login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !login.starts_with('-')
        && !login.ends_with('-');
    if valid { Ok(login) } else { Err("This is not a valid GitHub login.".to_string()) }
}

fn validate_role(value: &str) -> Result<&str, String> {
    ROLES.iter().copied().find(|role| *role == value).ok_or_else(|| "Unknown role.".to_string())
}

async fn create(admin: AdminUser, State(state): State<AdminState>, Form(input): Form<AdminForm>) -> Result<Response, AdminError> {
    admin.require_owner()?;
    let valid = validate_login(&input.github_login).and_then(|login| Ok((login, validate_role(&input.role)?)));
    let (login, role) = match valid {
        Ok(valid) => valid,
        Err(error) => return form(admin, None, input, Some(error)),
    };

    let mut tx = state.pool.begin().await?;
    // 409 when the login already exists
    let created = sqlx::query_as::<_, AdminRecord>("
INSERT INTO \"admin_user\" (github_login, role, active)
VALUES ($1, $2, TRUE)
RETURNING github_login, role, active
")
        .bind(&login)
        .bind(role)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, &login, Action::Create, None, Some(&created)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(login): Path<String>,
    Form(input): Form<AdminForm>,
) -> Result<Response, AdminError> {
    admin.require_owner()?;
    let active = input.active == "true";
    let valid = validate_role(&input.role).and_then(|role| {
        // an owner locking themselves out would leave nobody able to fix it
        if login.eq_ignore_ascii_case(&admin.github_login) && (role != "owner" || !active) {
            Err("You can't remove your own owner role or disable yourself.".to_string())
        } else {
            Ok(role)
        }
    });
    let role = match valid {
        Ok(role) => role,
        Err(error) => return form(admin, Some(&login), input, Some(error)),
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, &login, true).await?;
    let after = sqlx::query_as::<_, AdminRecord>("
UPDATE \"admin_user\" SET role = $1, active = $2 WHERE github_login = $3
RETURNING github_login, role, active
")
        .bind(role)
        .bind(active)
        .bind(&before.github_login)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, &before.github_login, Action::Update, Some(&before), Some(&after)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch(conn: &mut PgConnection, login: &str, for_update: bool) -> Result<AdminRecord, AdminError> {
    let query = if for_update {
        "SELECT github_login, role, active FROM \"admin_user\" WHERE github_login = $1 FOR UPDATE"
    } else {
        "SELECT github_login, role, active FROM \"admin_user\" WHERE github_login = $1"
    };
    sqlx::query_as::<_, AdminRecord>(query)
        .bind(login)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}
