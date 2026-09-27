use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{
    Field, FormPage, ListPage, Row, artwork_choices, delete_action, drop_type_choices, optional, redirect_to, required,
};
use crate::admin::AdminState;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;

const ENTITY: &str = "drop";
const LIST_URL: &str = "/drops";

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/drops", get(list).post(create))
        .route("/drops/new", get(new_form))
        .route("/drops/{id}/edit", get(edit_form))
        .route("/drops/{id}", post(update))
        .route("/drops/{id}/delete", post(delete))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct DropRecord {
    id: i32,
    artwork_id: i32,
    type_id: i32,
    name: String,
    dir: String,
}

#[derive(Deserialize, Debug, Default)]
struct DropForm {
    artwork_id: i32,
    type_id: i32,
    name: String,
    dir: String,
}

#[derive(sqlx::FromRow)]
struct DropLine {
    id: i32,
    name: String,
    drop_type: String,
    artwork: String,
    tags: i64,
    dir: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let drops = sqlx::query_as::<_, DropLine>("
SELECT d.id, d.name, dt.name AS drop_type, aw.name AS artwork,
       (SELECT COUNT(*) FROM \"tag\" t WHERE t.drop_id = d.id) AS tags,
       d.dir
FROM \"drop\" d
JOIN \"artwork\" aw ON aw.id = d.artwork_id
JOIN \"drop_type\" dt ON dt.id = d.type_id
ORDER BY d.name
")
        .fetch_all(&state.pool)
        .await?;

    let rows = drops.into_iter().map(|drop| Row {
        edit_url: Some(format!("/drops/{}/edit", drop.id)),
        actions: delete_action(&admin, format!("/drops/{}/delete", drop.id)),
        cells: vec![drop.id.to_string(), drop.name, drop.drop_type, drop.artwork, drop.tags.to_string(), drop.dir],
    }).collect();

    ListPage {
        admin,
        title: "Drops",
        note: None,
        new_url: Some("/drops/new"),
        columns: vec!["Id", "Name", "Type", "Artwork", "Tags", "Directory"],
        rows,
    }.respond()
}

async fn form(
    state: &AdminState,
    admin: AdminUser,
    id: Option<i32>,
    form: DropForm,
    error: Option<String>,
) -> Result<Response, AdminError> {
    FormPage {
        admin,
        title: id.map_or("New drop".to_string(), |id| format!("Edit drop #{id}")),
        action: id.map_or(LIST_URL.to_string(), |id| format!("/drops/{id}")),
        cancel_url: LIST_URL,
        fields: vec![
            Field::text("name", "Name", form.name, 255),
            Field::select("artwork_id", "Artwork", form.artwork_id, artwork_choices(&state.pool).await?),
            Field::select("type_id", "Type", form.type_id, drop_type_choices(&state.pool).await?),
            Field::text("dir", "Directory", form.dir, 128)
                .optional()
                .help("Directory of the drop's files on the web server."),
        ],
        error,
    }.respond()
}

async fn new_form(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    form(&state, admin, None, DropForm::default(), None).await
}

async fn edit_form(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i32>) -> Result<Response, AdminError> {
    let mut conn = state.pool.acquire().await?;
    let drop = fetch(&mut conn, id, false).await?;
    let input = DropForm { artwork_id: drop.artwork_id, type_id: drop.type_id, name: drop.name, dir: drop.dir };
    form(&state, admin, Some(id), input, None).await
}

fn validate(input: &DropForm) -> Result<(String, String), String> {
    Ok((required(&input.name, "Name", 255)?, optional(&input.dir, "Directory", 128)?))
}

async fn create(admin: AdminUser, State(state): State<AdminState>, Form(input): Form<DropForm>) -> Result<Response, AdminError> {
    let (name, dir) = match validate(&input) {
        Ok(valid) => valid,
        Err(error) => return form(&state, admin, None, input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let drop = sqlx::query_as::<_, DropRecord>("
INSERT INTO \"drop\" (artwork_id, type_id, name, dir)
VALUES ($1, $2, $3, $4)
RETURNING id, artwork_id, type_id, name, dir
")
        .bind(input.artwork_id)
        .bind(input.type_id)
        .bind(&name)
        .bind(&dir)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, drop.id, Action::Create, None, Some(&drop)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
    Form(input): Form<DropForm>,
) -> Result<Response, AdminError> {
    let (name, dir) = match validate(&input) {
        Ok(valid) => valid,
        Err(error) => return form(&state, admin, Some(id), input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    let after = sqlx::query_as::<_, DropRecord>("
UPDATE \"drop\"
SET artwork_id = $1, type_id = $2, name = $3, dir = $4
WHERE id = $5
RETURNING id, artwork_id, type_id, name, dir
")
        .bind(input.artwork_id)
        .bind(input.type_id)
        .bind(&name)
        .bind(&dir)
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
    // refused (409) while tags, playlists or redirects still reference the drop
    sqlx::query("DELETE FROM \"drop\" WHERE id = $1").bind(id).execute(&mut *tx).await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch(conn: &mut PgConnection, id: i32, for_update: bool) -> Result<DropRecord, AdminError> {
    let query = if for_update {
        "SELECT id, artwork_id, type_id, name, dir FROM \"drop\" WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, artwork_id, type_id, name, dir FROM \"drop\" WHERE id = $1"
    };
    sqlx::query_as::<_, DropRecord>(query)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}
