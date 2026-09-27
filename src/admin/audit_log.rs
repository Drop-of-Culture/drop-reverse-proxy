//! Read-only view of the audit log, optionally filtered: /audit?entity=drop&entity_id=3

use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{ListPage, Row};
use crate::admin::AdminState;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

const LIST_LIMIT: i64 = 200;

pub fn routes() -> Router<AdminState> {
    Router::new().route("/audit", get(list))
}

#[derive(Deserialize, Debug)]
struct AuditFilter {
    entity: Option<String>,
    entity_id: Option<String>,
}

#[derive(sqlx::FromRow)]
struct AuditLine {
    at: String,
    github_login: String,
    action: String,
    entity: String,
    entity_id: String,
    before: Option<String>,
    after: Option<String>,
}

async fn list(
    admin: AdminUser,
    State(state): State<AdminState>,
    Query(filter): Query<AuditFilter>,
) -> Result<Response, AdminError> {
    let entity = filter.entity.filter(|entity| !entity.is_empty());
    let entity_id = filter.entity_id.filter(|id| !id.is_empty());
    let lines = sqlx::query_as::<_, AuditLine>("
SELECT to_char(create_date, 'YYYY-MM-DD HH24:MI:SS') AS at, github_login, action, entity, entity_id,
       before::text AS before, after::text AS after
FROM \"audit_log\"
WHERE ($1::text IS NULL OR entity = $1)
  AND ($2::text IS NULL OR entity_id = $2)
ORDER BY id DESC
LIMIT $3
")
        .bind(&entity)
        .bind(&entity_id)
        .bind(LIST_LIMIT)
        .fetch_all(&state.pool)
        .await?;

    let rows = lines.into_iter().map(|line| Row {
        cells: vec![
            line.at,
            line.github_login,
            line.action,
            line.entity,
            line.entity_id,
            line.before.unwrap_or_default(),
            line.after.unwrap_or_default(),
        ],
        edit_url: None,
        actions: Vec::new(),
    }).collect();

    let scope = match (&entity, &entity_id) {
        (Some(entity), Some(id)) => format!(" for {entity} {id}"),
        (Some(entity), None) => format!(" for {entity}"),
        _ => String::new(),
    };
    ListPage {
        admin,
        title: "Audit log",
        note: Some(format!("Latest {LIST_LIMIT} changes{scope}, newest first.")),
        new_url: None,
        columns: vec!["When (UTC)", "Who", "Action", "Entity", "Id", "Before", "After"],
        rows,
    }.respond()
}
