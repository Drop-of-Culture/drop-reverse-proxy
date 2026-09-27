use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{Field, FormPage, ListPage, Row, delete_action, drop_choices, redirect_to, required};
use crate::admin::AdminState;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;

const ENTITY: &str = "redirect";
const LIST_URL: &str = "/redirects";

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/redirects", get(list).post(create))
        .route("/redirects/new", get(new_form))
        .route("/redirects/{id}/edit", get(edit_form))
        .route("/redirects/{id}", post(update))
        .route("/redirects/{id}/delete", post(delete))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct RedirectRecord {
    id: i32,
    drop_id: i32,
    name: String,
    link: String,
}

#[derive(Deserialize, Debug, Default)]
struct RedirectForm {
    drop_id: i32,
    name: String,
    link: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let redirects = sqlx::query_as::<_, (i32, String, String, String)>("
SELECT r.id, r.name, d.name, r.link
FROM \"redirect\" r
JOIN \"drop\" d ON d.id = r.drop_id
ORDER BY r.name
")
        .fetch_all(&state.pool)
        .await?;

    let rows = redirects.into_iter().map(|(id, name, drop, link)| Row {
        cells: vec![id.to_string(), name, drop, link],
        edit_url: Some(format!("/redirects/{id}/edit")),
        actions: delete_action(&admin, format!("/redirects/{id}/delete")),
    }).collect();

    ListPage {
        admin,
        title: "Redirects",
        note: Some("Visitors of a tag whose drop has the redirect type are sent to the drop's link.".to_string()),
        new_url: Some("/redirects/new"),
        columns: vec!["Id", "Name", "Drop", "Link"],
        rows,
    }.respond()
}

async fn form(
    state: &AdminState,
    admin: AdminUser,
    id: Option<i32>,
    form: RedirectForm,
    error: Option<String>,
) -> Result<Response, AdminError> {
    FormPage {
        admin,
        title: id.map_or("New redirect".to_string(), |id| format!("Edit redirect #{id}")),
        action: id.map_or(LIST_URL.to_string(), |id| format!("/redirects/{id}")),
        cancel_url: LIST_URL,
        fields: vec![
            Field::text("name", "Name", form.name, 255),
            Field::select("drop_id", "Drop", form.drop_id, drop_choices(&state.pool).await?),
            Field::text("link", "Link", form.link, 2048).input_type("url").help("http:// or https:// address"),
        ],
        error,
    }.respond()
}

async fn new_form(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    form(&state, admin, None, RedirectForm::default(), None).await
}

async fn edit_form(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i32>) -> Result<Response, AdminError> {
    let mut conn = state.pool.acquire().await?;
    let redirect = fetch(&mut conn, id, false).await?;
    let input = RedirectForm { drop_id: redirect.drop_id, name: redirect.name, link: redirect.link };
    form(&state, admin, Some(id), input, None).await
}

fn validate(input: &RedirectForm) -> Result<(String, String), String> {
    let name = required(&input.name, "Name", 255)?;
    let link = required(&input.link, "Link", 2048)?;
    if !(link.starts_with("https://") || link.starts_with("http://")) {
        return Err("Link must start with http:// or https://.".to_string());
    }
    Ok((name, link))
}

async fn create(admin: AdminUser, State(state): State<AdminState>, Form(input): Form<RedirectForm>) -> Result<Response, AdminError> {
    let (name, link) = match validate(&input) {
        Ok(valid) => valid,
        Err(error) => return form(&state, admin, None, input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let redirect = sqlx::query_as::<_, RedirectRecord>("
INSERT INTO \"redirect\" (drop_id, name, link)
VALUES ($1, $2, $3)
RETURNING id, drop_id, name, link
")
        .bind(input.drop_id)
        .bind(&name)
        .bind(&link)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, redirect.id, Action::Create, None, Some(&redirect)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
    Form(input): Form<RedirectForm>,
) -> Result<Response, AdminError> {
    let (name, link) = match validate(&input) {
        Ok(valid) => valid,
        Err(error) => return form(&state, admin, Some(id), input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    let after = sqlx::query_as::<_, RedirectRecord>("
UPDATE \"redirect\"
SET drop_id = $1, name = $2, link = $3, update_date = CURRENT_TIMESTAMP
WHERE id = $4
RETURNING id, drop_id, name, link
")
        .bind(input.drop_id)
        .bind(&name)
        .bind(&link)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Update, Some(&before), Some(&after)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn delete(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i32>) -> Result<Response, AdminError> {
    admin.require_owner()?;

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    sqlx::query("DELETE FROM \"redirect\" WHERE id = $1").bind(id).execute(&mut *tx).await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch(conn: &mut PgConnection, id: i32, for_update: bool) -> Result<RedirectRecord, AdminError> {
    let query = if for_update {
        "SELECT id, drop_id, name, link FROM \"redirect\" WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, drop_id, name, link FROM \"redirect\" WHERE id = $1"
    };
    sqlx::query_as::<_, RedirectRecord>(query)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}
