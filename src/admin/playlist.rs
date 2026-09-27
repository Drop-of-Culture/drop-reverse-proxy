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

const ENTITY: &str = "playlist";
const LIST_URL: &str = "/playlists";

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/playlists", get(list).post(create))
        .route("/playlists/new", get(new_form))
        .route("/playlists/{id}/edit", get(edit_form))
        .route("/playlists/{id}", post(update))
        .route("/playlists/{id}/delete", post(delete))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct PlaylistRecord {
    id: i32,
    drop_id: i32,
    name: String,
}

#[derive(Deserialize, Debug, Default)]
struct PlaylistForm {
    drop_id: i32,
    name: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let playlists = sqlx::query_as::<_, (i32, String, String)>("
SELECT p.id, p.name, d.name
FROM \"playlist\" p
JOIN \"drop\" d ON d.id = p.drop_id
ORDER BY p.name
")
        .fetch_all(&state.pool)
        .await?;

    let rows = playlists.into_iter().map(|(id, name, drop)| Row {
        cells: vec![id.to_string(), name, drop],
        edit_url: Some(format!("/playlists/{id}/edit")),
        actions: delete_action(&admin, format!("/playlists/{id}/delete")),
    }).collect();

    ListPage {
        admin,
        title: "Playlists",
        note: None,
        new_url: Some("/playlists/new"),
        columns: vec!["Id", "Name", "Drop"],
        rows,
    }.respond()
}

async fn form(
    state: &AdminState,
    admin: AdminUser,
    id: Option<i32>,
    form: PlaylistForm,
    error: Option<String>,
) -> Result<Response, AdminError> {
    FormPage {
        admin,
        title: id.map_or("New playlist".to_string(), |id| format!("Edit playlist #{id}")),
        action: id.map_or(LIST_URL.to_string(), |id| format!("/playlists/{id}")),
        cancel_url: LIST_URL,
        fields: vec![
            Field::text("name", "Name", form.name, 255),
            Field::select("drop_id", "Drop", form.drop_id, drop_choices(&state.pool).await?),
        ],
        error,
    }.respond()
}

async fn new_form(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    form(&state, admin, None, PlaylistForm::default(), None).await
}

async fn edit_form(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i32>) -> Result<Response, AdminError> {
    let mut conn = state.pool.acquire().await?;
    let playlist = fetch(&mut conn, id, false).await?;
    form(&state, admin, Some(id), PlaylistForm { drop_id: playlist.drop_id, name: playlist.name }, None).await
}

async fn create(admin: AdminUser, State(state): State<AdminState>, Form(input): Form<PlaylistForm>) -> Result<Response, AdminError> {
    let name = match required(&input.name, "Name", 255) {
        Ok(name) => name,
        Err(error) => return form(&state, admin, None, input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let playlist = sqlx::query_as::<_, PlaylistRecord>("
INSERT INTO \"playlist\" (drop_id, name)
VALUES ($1, $2)
RETURNING id, drop_id, name
")
        .bind(input.drop_id)
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, playlist.id, Action::Create, None, Some(&playlist)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
    Form(input): Form<PlaylistForm>,
) -> Result<Response, AdminError> {
    let name = match required(&input.name, "Name", 255) {
        Ok(name) => name,
        Err(error) => return form(&state, admin, Some(id), input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    let after = sqlx::query_as::<_, PlaylistRecord>("
UPDATE \"playlist\"
SET drop_id = $1, name = $2, update_date = CURRENT_TIMESTAMP
WHERE id = $3
RETURNING id, drop_id, name
")
        .bind(input.drop_id)
        .bind(&name)
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
    sqlx::query("DELETE FROM \"playlist\" WHERE id = $1").bind(id).execute(&mut *tx).await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch(conn: &mut PgConnection, id: i32, for_update: bool) -> Result<PlaylistRecord, AdminError> {
    let query = if for_update {
        "SELECT id, drop_id, name FROM \"playlist\" WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, drop_id, name FROM \"playlist\" WHERE id = $1"
    };
    sqlx::query_as::<_, PlaylistRecord>(query)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}
