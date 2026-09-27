//! Artist CRUD. The other content entities follow the same pattern.

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

const ENTITY: &str = "artist";
const LIST_URL: &str = "/artists";

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/artists", get(list).post(create))
        .route("/artists/new", get(new_form))
        .route("/artists/{id}/edit", get(edit_form))
        .route("/artists/{id}", post(update))
        .route("/artists/{id}/delete", post(delete))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct ArtistRecord {
    id: i32,
    name: String,
}

#[derive(Deserialize, Debug)]
struct ArtistForm {
    name: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let artists = sqlx::query_as::<_, (i32, String, i64)>("
SELECT a.id, a.name, COUNT(aw.id)
FROM \"artist\" a
LEFT JOIN \"artwork\" aw ON aw.artist_id = a.id
GROUP BY a.id, a.name
ORDER BY a.name
")
        .fetch_all(&state.pool)
        .await?;

    let rows = artists.into_iter().map(|(id, name, artworks)| Row {
        cells: vec![id.to_string(), name, artworks.to_string()],
        edit_url: Some(format!("/artists/{id}/edit")),
        actions: delete_action(&admin, format!("/artists/{id}/delete")),
    }).collect();

    ListPage {
        admin,
        title: "Artists",
        note: None,
        new_url: Some("/artists/new"),
        columns: vec!["Id", "Name", "Artworks"],
        rows,
    }.respond()
}

fn form(admin: AdminUser, id: Option<i32>, form: ArtistForm, error: Option<String>) -> FormPage {
    FormPage {
        admin,
        title: id.map_or("New artist".to_string(), |id| format!("Edit artist #{id}")),
        action: id.map_or(LIST_URL.to_string(), |id| format!("/artists/{id}")),
        cancel_url: LIST_URL,
        fields: vec![Field::text("name", "Name", form.name, 255)],
        error,
    }
}

async fn new_form(admin: AdminUser) -> Result<Response, AdminError> {
    form(admin, None, ArtistForm { name: String::new() }, None).respond()
}

async fn edit_form(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i32>) -> Result<Response, AdminError> {
    let mut conn = state.pool.acquire().await?;
    let artist = fetch(&mut conn, id, false).await?;
    form(admin, Some(id), ArtistForm { name: artist.name }, None).respond()
}

async fn create(admin: AdminUser, State(state): State<AdminState>, Form(input): Form<ArtistForm>) -> Result<Response, AdminError> {
    let name = match required(&input.name, "Name", 255) {
        Ok(name) => name,
        Err(error) => return form(admin, None, input, Some(error)).respond(),
    };

    let mut tx = state.pool.begin().await?;
    let artist = sqlx::query_as::<_, ArtistRecord>("INSERT INTO \"artist\" (name) VALUES ($1) RETURNING id, name")
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, artist.id, Action::Create, None, Some(&artist)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
    Form(input): Form<ArtistForm>,
) -> Result<Response, AdminError> {
    let name = match required(&input.name, "Name", 255) {
        Ok(name) => name,
        Err(error) => return form(admin, Some(id), input, Some(error)).respond(),
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    let after = sqlx::query_as::<_, ArtistRecord>("UPDATE \"artist\" SET name = $1 WHERE id = $2 RETURNING id, name")
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
    // refused (409) while artworks still reference the artist
    sqlx::query("DELETE FROM \"artist\" WHERE id = $1").bind(id).execute(&mut *tx).await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch(conn: &mut PgConnection, id: i32, for_update: bool) -> Result<ArtistRecord, AdminError> {
    let query = if for_update {
        "SELECT id, name FROM \"artist\" WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, name FROM \"artist\" WHERE id = $1"
    };
    sqlx::query_as::<_, ArtistRecord>(query)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}

