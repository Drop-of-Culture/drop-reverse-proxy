use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{Field, FormPage, ListPage, Row, artist_choices, delete_action, redirect_to, required};
use crate::admin::AdminState;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;

const ENTITY: &str = "artwork";
const LIST_URL: &str = "/artworks";

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/artworks", get(list).post(create))
        .route("/artworks/new", get(new_form))
        .route("/artworks/{id}/edit", get(edit_form))
        .route("/artworks/{id}", post(update))
        .route("/artworks/{id}/delete", post(delete))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct ArtworkRecord {
    id: i32,
    artist_id: i32,
    name: String,
}

#[derive(Deserialize, Debug, Default)]
struct ArtworkForm {
    artist_id: i32,
    name: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let artworks = sqlx::query_as::<_, (i32, String, String, i64)>("
SELECT aw.id, aw.name, a.name, (SELECT COUNT(*) FROM \"drop\" d WHERE d.artwork_id = aw.id)
FROM \"artwork\" aw
JOIN \"artist\" a ON a.id = aw.artist_id
ORDER BY aw.name
")
        .fetch_all(&state.pool)
        .await?;

    let rows = artworks.into_iter().map(|(id, name, artist, drops)| Row {
        cells: vec![id.to_string(), name, artist, drops.to_string()],
        edit_url: Some(format!("/artworks/{id}/edit")),
        actions: delete_action(&admin, format!("/artworks/{id}/delete")),
    }).collect();

    ListPage {
        admin,
        title: "Artworks",
        note: None,
        new_url: Some("/artworks/new"),
        columns: vec!["Id", "Name", "Artist", "Drops"],
        rows,
    }.respond()
}

async fn form(
    state: &AdminState,
    admin: AdminUser,
    id: Option<i32>,
    form: ArtworkForm,
    error: Option<String>,
) -> Result<Response, AdminError> {
    FormPage {
        admin,
        title: id.map_or("New artwork".to_string(), |id| format!("Edit artwork #{id}")),
        action: id.map_or(LIST_URL.to_string(), |id| format!("/artworks/{id}")),
        cancel_url: LIST_URL,
        fields: vec![
            Field::text("name", "Name", form.name, 255),
            Field::select("artist_id", "Artist", form.artist_id, artist_choices(&state.pool).await?),
        ],
        error,
    }.respond()
}

async fn new_form(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    form(&state, admin, None, ArtworkForm::default(), None).await
}

async fn edit_form(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i32>) -> Result<Response, AdminError> {
    let mut conn = state.pool.acquire().await?;
    let artwork = fetch(&mut conn, id, false).await?;
    let input = ArtworkForm { artist_id: artwork.artist_id, name: artwork.name };
    form(&state, admin, Some(id), input, None).await
}

async fn create(admin: AdminUser, State(state): State<AdminState>, Form(input): Form<ArtworkForm>) -> Result<Response, AdminError> {
    let name = match required(&input.name, "Name", 255) {
        Ok(name) => name,
        Err(error) => return form(&state, admin, None, input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let artwork = sqlx::query_as::<_, ArtworkRecord>("
INSERT INTO \"artwork\" (artist_id, name)
VALUES ($1, $2)
RETURNING id, artist_id, name
")
        .bind(input.artist_id)
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, artwork.id, Action::Create, None, Some(&artwork)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
    Form(input): Form<ArtworkForm>,
) -> Result<Response, AdminError> {
    let name = match required(&input.name, "Name", 255) {
        Ok(name) => name,
        Err(error) => return form(&state, admin, Some(id), input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    let after = sqlx::query_as::<_, ArtworkRecord>("
UPDATE \"artwork\"
SET artist_id = $1, name = $2, update_date = CURRENT_TIMESTAMP
WHERE id = $3
RETURNING id, artist_id, name
")
        .bind(input.artist_id)
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
    sqlx::query("DELETE FROM \"artwork\" WHERE id = $1").bind(id).execute(&mut *tx).await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch(conn: &mut PgConnection, id: i32, for_update: bool) -> Result<ArtworkRecord, AdminError> {
    let query = if for_update {
        "SELECT id, artist_id, name FROM \"artwork\" WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, artist_id, name FROM \"artwork\" WHERE id = $1"
    };
    sqlx::query_as::<_, ArtworkRecord>(query)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}
