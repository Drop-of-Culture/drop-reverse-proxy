//! Tokens are issued to visitors when they open a tag: they can only be listed and revoked.

use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{ListPage, Row, RowAction, redirect_to};
use crate::admin::AdminState;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde::Serialize;
use uuid::Uuid;

const ENTITY: &str = "token";
const LIST_URL: &str = "/tokens";
const LIST_LIMIT: i64 = 500;

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/tokens", get(list))
        .route("/tokens/{id}/delete", post(revoke))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct TokenRecord {
    id: String,
    tag_id: i32,
}

#[derive(sqlx::FromRow)]
struct TokenLine {
    id: Uuid,
    tag: String,
    drop_name: String,
    created: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    let tokens = sqlx::query_as::<_, TokenLine>("
SELECT tk.id, t.name AS tag, d.name AS drop_name, to_char(tk.create_date, 'YYYY-MM-DD HH24:MI') AS created
FROM \"token\" tk
JOIN \"tag\" t ON t.id = tk.tag_id
JOIN \"drop\" d ON d.id = t.drop_id
ORDER BY tk.create_date DESC
LIMIT $1
")
        .bind(LIST_LIMIT)
        .fetch_all(&state.pool)
        .await?;

    let rows = tokens.into_iter().map(|token| Row {
        actions: vec![RowAction { label: "Revoke", url: format!("/tokens/{}/delete", token.id), danger: true }],
        cells: vec![token.id.to_string(), token.tag, token.drop_name, token.created],
        edit_url: None,
    }).collect();

    ListPage {
        admin,
        title: "Tokens",
        note: Some(format!("Issued to visitors opening a tag, newest first ({LIST_LIMIT} at most). A revoked token stops working immediately.")),
        new_url: None,
        columns: vec!["Token", "Tag", "Drop", "Created"],
        rows,
    }.respond()
}

async fn revoke(admin: AdminUser, State(state): State<AdminState>, Path(id): Path<String>) -> Result<Response, AdminError> {
    let id = Uuid::parse_str(&id).map_err(|_| AdminError::NotFound)?;

    let mut tx = state.pool.begin().await?;
    let before = sqlx::query_as::<_, TokenRecord>("DELETE FROM \"token\" WHERE id = $1 RETURNING id::text AS id, tag_id")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AdminError::NotFound)?;
    audit::record_change(&mut tx, &admin, ENTITY, id, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}
