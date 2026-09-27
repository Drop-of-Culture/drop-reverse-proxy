use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{Field, FormPage, ListPage, Row, delete_action, drop_choices, optional, redirect_to};
use crate::admin::AdminState;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

const ENTITY: &str = "tag";
const LIST_URL: &str = "/tags";
const NAME_MAX_LEN: usize = 255;
const GENERATED_NAME_LEN: usize = 16;

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/tags", get(list).post(create))
        .route("/tags/new", get(new_form))
        .route("/tags/{id}/edit", get(edit_form))
        .route("/tags/{id}", post(update))
        .route("/tags/{id}/delete", post(delete))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct TagRecord {
    id: i32,
    drop_id: i32,
    name: String,
}

#[derive(Deserialize, Debug, Default)]
struct TagForm {
    drop_id: i32,
    name: String,
}

#[derive(sqlx::FromRow)]
struct TagLine {
    id: i32,
    name: String,
    drop_name: String,
    tokens: i64,
    created: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let tags = sqlx::query_as::<_, TagLine>("
SELECT t.id, t.name, d.name AS drop_name,
       (SELECT COUNT(*) FROM \"token\" tk WHERE tk.tag_id = t.id) AS tokens,
       to_char(t.create_date, 'YYYY-MM-DD HH24:MI') AS created
FROM \"tag\" t
JOIN \"drop\" d ON d.id = t.drop_id
ORDER BY t.create_date DESC
")
        .fetch_all(&state.pool)
        .await?;

    let rows = tags.into_iter().map(|tag| Row {
        edit_url: Some(format!("/tags/{}/edit", tag.id)),
        actions: delete_action(&admin, format!("/tags/{}/delete", tag.id)),
        cells: vec![tag.id.to_string(), format!("/tag/{}", tag.name), tag.drop_name, tag.tokens.to_string(), tag.created],
    }).collect();

    ListPage {
        admin,
        title: "Tags",
        note: Some("A tag is the public link to a drop. Deleting a tag needs its tokens to be revoked first.".to_string()),
        new_url: Some("/tags/new"),
        columns: vec!["Id", "Link", "Drop", "Tokens", "Created"],
        rows,
    }.respond()
}

async fn form(
    state: &AdminState,
    admin: AdminUser,
    id: Option<i32>,
    form: TagForm,
    error: Option<String>,
) -> Result<Response, AdminError> {
    let mut name = Field::text("name", "Name", form.name, NAME_MAX_LEN)
        .help("Letters, digits, - and _ only. Changing it breaks links already shared.");
    if id.is_none() {
        name = name.optional().help("Leave empty to generate a random one. Letters, digits, - and _ only.");
    }
    FormPage {
        admin,
        title: id.map_or("New tag".to_string(), |id| format!("Edit tag #{id}")),
        action: id.map_or(LIST_URL.to_string(), |id| format!("/tags/{id}")),
        cancel_url: LIST_URL,
        fields: vec![name, Field::select("drop_id", "Drop", form.drop_id, drop_choices(&state.pool).await?)],
        error,
    }.respond()
}

async fn new_form(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    form(&state, admin, None, TagForm::default(), None).await
}

async fn edit_form(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<i32>) -> Result<Response, AdminError> {
    let mut conn = state.pool.acquire().await?;
    let tag = fetch(&mut conn, id, false).await?;
    form(&state, admin, Some(id), TagForm { drop_id: tag.drop_id, name: tag.name }, None).await
}

/// The name ends up as the last segment of the public `/tag/{name}` url.
fn validate_name(value: &str, generate_if_empty: bool) -> Result<String, String> {
    let name = optional(value, "Name", NAME_MAX_LEN)?;
    if name.is_empty() {
        return if generate_if_empty { Ok(random_name()) } else { Err("Name is required.".to_string()) };
    }
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Ok(name)
    } else {
        Err("Name can only contain letters, digits, - and _.".to_string())
    }
}

/// Lowercase letters, like the existing tags.
fn random_name() -> String {
    Uuid::new_v4()
        .as_bytes()
        .iter()
        .take(GENERATED_NAME_LEN)
        .map(|byte| (b'a' + byte % 26) as char)
        .collect()
}

async fn create(admin: AdminUser, State(state): State<AdminState>, Form(input): Form<TagForm>) -> Result<Response, AdminError> {
    let name = match validate_name(&input.name, true) {
        Ok(name) => name,
        Err(error) => return form(&state, admin, None, input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let tag = sqlx::query_as::<_, TagRecord>("
INSERT INTO \"tag\" (drop_id, name)
VALUES ($1, $2)
RETURNING id, drop_id, name
")
        .bind(input.drop_id)
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, tag.id, Action::Create, None, Some(&tag)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn update(
    admin: AdminUser,
    State(state): State<AdminState>,
    Path(id): Path<i32>,
    Form(input): Form<TagForm>,
) -> Result<Response, AdminError> {
    let name = match validate_name(&input.name, false) {
        Ok(name) => name,
        Err(error) => return form(&state, admin, Some(id), input, Some(error)).await,
    };

    let mut tx = state.pool.begin().await?;
    let before = fetch(&mut tx, id, true).await?;
    let after = sqlx::query_as::<_, TagRecord>("
UPDATE \"tag\"
SET drop_id = $1, name = $2
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
    // refused (409) while tokens still reference the tag
    sqlx::query("DELETE FROM \"tag\" WHERE id = $1").bind(id).execute(&mut *tx).await?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch(conn: &mut PgConnection, id: i32, for_update: bool) -> Result<TagRecord, AdminError> {
    let query = if for_update {
        "SELECT id, drop_id, name FROM \"tag\" WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, drop_id, name FROM \"tag\" WHERE id = $1"
    };
    sqlx::query_as::<_, TagRecord>(query)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_names_must_be_url_safe() {
        assert_eq!(validate_name(" summer-drop_2 ", false), Ok("summer-drop_2".to_string()));
        assert!(validate_name("a/b", false).is_err());
        assert!(validate_name("a b", false).is_err());
        assert!(validate_name("été", false).is_err());
        assert!(validate_name("", false).is_err());
    }

    #[test]
    fn empty_name_is_generated_on_create() {
        let name = validate_name("  ", true).unwrap();
        assert_eq!(name.len(), GENERATED_NAME_LEN);
        assert!(name.chars().all(|c| c.is_ascii_lowercase()));
        assert_ne!(name, validate_name("", true).unwrap());
    }
}
