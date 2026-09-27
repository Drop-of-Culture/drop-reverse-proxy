//! Artist CRUD. Serves as the pattern for the other entities.

use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::{AdminState, render};
use crate::repository::artist::Artist;
use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

const ENTITY: &str = "artist";
const NAME_MAX_LEN: usize = 255;

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/artists", get(list).post(create))
        .route("/artists/new", get(new_form))
        .route("/artists/{id}/edit", get(edit_form))
        .route("/artists/{id}", post(update))
        .route("/artists/{id}/delete", post(delete))
}

#[derive(sqlx::FromRow, Debug)]
pub struct ArtistRow {
    pub id: i32,
    pub name: String,
    pub artwork_count: i64,
}

#[derive(Deserialize, Debug)]
pub struct ArtistForm {
    name: String,
}

impl ArtistForm {
    fn validated_name(&self) -> Result<String, String> {
        let name = self.name.trim();
        if name.is_empty() {
            Err("Name is required.".to_string())
        } else if name.chars().count() > NAME_MAX_LEN {
            Err(format!("Name must be at most {NAME_MAX_LEN} characters."))
        } else {
            Ok(name.to_string())
        }
    }
}

#[derive(Template)]
#[template(path = "admin/artist/list.html")]
struct ListTemplate {
    admin: AdminUser,
    artists: Vec<ArtistRow>,
}

#[derive(Template)]
#[template(path = "admin/artist/form.html")]
struct FormTemplate {
    admin: AdminUser,
    artist_id: Option<i32>,
    name: String,
    error: Option<String>,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let artists = sqlx::query_as::<_, ArtistRow>("
SELECT a.id, a.name, COUNT(aw.id) AS artwork_count
FROM \"artist\" a
LEFT JOIN \"artwork\" aw ON aw.artist_id = a.id
GROUP BY a.id, a.name
ORDER BY a.name
")
        .fetch_all(&state.pool)
        .await?;
    Ok(render(&ListTemplate { admin, artists })?.into_response())
}

async fn new_form(admin: AdminUser) -> Result<Response, AdminError> {
    Ok(render(&FormTemplate { admin, artist_id: None, name: String::new(), error: None })?.into_response())
}

async fn edit_form(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
) -> Result<Response, AdminError> {
    let mut conn = state.pool.acquire().await?;
    let artist = fetch(&mut conn, id, false).await?;
    Ok(render(&FormTemplate {
        admin,
        artist_id: Some(artist.id()),
        name: artist.name().to_string(),
        error: None,
    })?.into_response())
}

async fn create(
    admin: AdminUser,
    State(state): State<AdminState>,
    Form(form): Form<ArtistForm>,
) -> Result<Response, AdminError> {
    let name = match form.validated_name() {
        Ok(name) => name,
        Err(error) => return invalid_form(admin, None, form.name, error),
    };

    let mut tx = state.pool.begin().await?;
    let artist = sqlx::query_as::<_, Artist>("
INSERT INTO \"artist\" (name)
VALUES ($1)
RETURNING id, name
")
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
    audit::record(&mut tx, &admin, ENTITY, &artist.id().to_string(), Action::Create, None, Some(to_json(&artist))).await?;
    tx.commit().await?;

    Ok(Redirect::to("/artists").into_response())
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
    Form(form): Form<ArtistForm>,
) -> Result<Response, AdminError> {
    let name = match form.validated_name() {
        Ok(name) => name,
        Err(error) => return invalid_form(admin, Some(id), form.name, error),
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    let after = sqlx::query_as::<_, Artist>("
UPDATE \"artist\"
SET name = $1
WHERE id = $2
RETURNING id, name
")
        .bind(&name)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    audit::record(&mut tx, &admin, ENTITY, &id.to_string(), Action::Update, Some(to_json(&before)), Some(to_json(&after))).await?;
    tx.commit().await?;

    Ok(Redirect::to("/artists").into_response())
}

async fn delete(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
) -> Result<Response, AdminError> {
    admin.require_owner()?;

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    // fails with a Conflict while artworks still reference the artist
    sqlx::query("DELETE FROM \"artist\" WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit::record(&mut tx, &admin, ENTITY, &id.to_string(), Action::Delete, Some(to_json(&before)), None).await?;
    tx.commit().await?;

    Ok(Redirect::to("/artists").into_response())
}

async fn fetch(conn: &mut PgConnection, id: i32, for_update: bool) -> Result<Artist, AdminError> {
    let query = if for_update {
        "SELECT id, name FROM \"artist\" WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, name FROM \"artist\" WHERE id = $1"
    };
    sqlx::query_as::<_, Artist>(query)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}

fn invalid_form(admin: AdminUser, artist_id: Option<i32>, name: String, error: String) -> Result<Response, AdminError> {
    let page = render(&FormTemplate { admin, artist_id, name, error: Some(error) })?;
    Ok((StatusCode::UNPROCESSABLE_ENTITY, page).into_response())
}

fn to_json(artist: &Artist) -> Value {
    json!({ "id": artist.id(), "name": artist.name() })
}
