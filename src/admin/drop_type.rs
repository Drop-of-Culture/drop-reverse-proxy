//! Drop types are codes the proxy relies on (e.g. 2 = redirect): only owners change them.

use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{Field, FormPage, ListPage, Row, delete_action, redirect_to, required};
use crate::admin::AdminState;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;

const ENTITY: &str = "drop_type";
const LIST_URL: &str = "/drop-types";

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/drop-types", get(list).post(create))
        .route("/drop-types/new", get(new_form))
        .route("/drop-types/{id}/edit", get(edit_form))
        .route("/drop-types/{id}", post(update))
        .route("/drop-types/{id}/delete", post(delete))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct DropTypeRecord {
    id: i16,
    name: String,
}

/// `id` is only sent on creation: it can't change afterwards.
#[derive(Deserialize, Debug, Default)]
struct DropTypeForm {
    #[serde(default)]
    id: String,
    name: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let drop_types = sqlx::query_as::<_, (i16, String, i64)>("
SELECT dt.id, dt.name, (SELECT COUNT(*) FROM \"drop\" d WHERE d.type_id = dt.id)
FROM \"drop_type\" dt
ORDER BY dt.id
")
        .fetch_all(&state.pool)
        .await?;

    let owner = admin.is_owner();
    let rows = drop_types.into_iter().map(|(id, name, drops)| Row {
        cells: vec![id.to_string(), name, drops.to_string()],
        edit_url: owner.then(|| format!("/drop-types/{id}/edit")),
        actions: delete_action(&admin, format!("/drop-types/{id}/delete")),
    }).collect();

    ListPage {
        admin,
        title: "Drop types",
        note: Some("The proxy relies on these ids (0 = audio playlist, 2 = redirect): only owners can change them.".to_string()),
        new_url: owner.then_some("/drop-types/new"),
        columns: vec!["Id", "Name", "Drops"],
        rows,
    }.respond()
}

fn form(admin: AdminUser, id: Option<i16>, form: DropTypeForm, error: Option<String>) -> Result<Response, AdminError> {
    let mut fields = Vec::new();
    if id.is_none() {
        fields.push(Field::text("id", "Id", form.id, 6).input_type("number").help("Number used by the code, can't be changed later."));
    }
    fields.push(Field::text("name", "Name", form.name, 127));
    FormPage {
        admin,
        title: id.map_or("New drop type".to_string(), |id| format!("Edit drop type {id}")),
        action: id.map_or(LIST_URL.to_string(), |id| format!("/drop-types/{id}")),
        cancel_url: LIST_URL,
        fields,
        error,
    }.respond()
}

async fn new_form(admin: AdminUser) -> Result<Response, AdminError> {
    admin.require_owner()?;
    form(admin, None, DropTypeForm::default(), None)
}

async fn edit_form(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i16>) -> Result<Response, AdminError> {
    admin.require_owner()?;
    let mut conn = state.pool.acquire().await?;
    let drop_type = fetch(&mut conn, id, false).await?;
    form(admin, Some(id), DropTypeForm { id: id.to_string(), name: drop_type.name }, None)
}

async fn create(admin: AdminUser, State(state): State<AdminState>, Form(input): Form<DropTypeForm>) -> Result<Response, AdminError> {
    admin.require_owner()?;
    let valid = input.id.trim().parse::<i16>()
        .ok()
        .filter(|id| *id >= 0)
        .ok_or_else(|| "Id must be a number between 0 and 32767.".to_string())
        .and_then(|id| Ok((id, required(&input.name, "Name", 127)?)));
    let (id, name) = match valid {
        Ok(valid) => valid,
        Err(error) => return form(admin, None, input, Some(error)),
    };

    let mut tx = state.pool.begin().await?;
    let drop_type = sqlx::query_as::<_, DropTypeRecord>("INSERT INTO \"drop_type\" (id, name) VALUES ($1, $2) RETURNING id, name")
        .bind(id)
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Create, None, Some(&drop_type)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i16>,
    Form(input): Form<DropTypeForm>,
) -> Result<Response, AdminError> {
    admin.require_owner()?;
    let name = match required(&input.name, "Name", 127) {
        Ok(name) => name,
        Err(error) => return form(admin, Some(id), input, Some(error)),
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    let after = sqlx::query_as::<_, DropTypeRecord>("UPDATE \"drop_type\" SET name = $1 WHERE id = $2 RETURNING id, name")
        .bind(&name)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Update, Some(&before), Some(&after)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn delete(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i16>) -> Result<Response, AdminError> {
    admin.require_owner()?;

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    // refused (409) while drops still have this type
    sqlx::query("DELETE FROM \"drop_type\" WHERE id = $1").bind(id).execute(&mut *tx).await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch(conn: &mut PgConnection, id: i16, for_update: bool) -> Result<DropTypeRecord, AdminError> {
    let query = if for_update {
        "SELECT id, name FROM \"drop_type\" WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, name FROM \"drop_type\" WHERE id = $1"
    };
    sqlx::query_as::<_, DropTypeRecord>(query)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}
